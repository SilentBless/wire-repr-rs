#[cfg(test)]
mod limit;
pub mod stack;
#[cfg(test)]
mod tail;
#[cfg(test)]
mod vtable;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use capstone::arch::ArchOperand;
use capstone::arch::arm64::Arm64OperandType;
use capstone::arch::x86::{X86OperandType, X86Reg};
use capstone::prelude::*;
use object::{
    Object, ObjectSection, ObjectSymbol, Relocation, RelocationFlags, RelocationKind,
    RelocationTarget, SectionKind, SymbolKind,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
const ALLOCATION_MARKERS: &[&str] = &[
    "__rust_alloc",
    "__rust_realloc",
    "__rust_dealloc",
    "exchange_malloc",
];
const PANIC_MARKERS: &[&str] = &[
    "panic",
    "bounds_check",
    "slice_index",
    "copy_from_slice",
    "assert_failed",
];
const MAX_REACHABLE_FUNCTIONS: usize = 256;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Metrics {
    pub text_bytes: u64,
    pub instructions: u64,
    pub branches: u64,
    pub direct_calls: u64,
    pub tail_calls: u64,
    pub indirect_calls: u64,
    pub loads: u64,
    pub stores: u64,
    pub panic_paths: u64,
    pub vtable_references: u64,
    pub allocation_symbols: u64,
    pub reachable_functions: u64,
    pub transitive_instructions: u64,
    pub transitive_branches: u64,
    pub transitive_direct_calls: u64,
    pub transitive_tail_calls: u64,
    pub transitive_indirect_calls: u64,
    pub transitive_loads: u64,
    pub transitive_stores: u64,
    pub transitive_panic_paths: u64,
    pub transitive_vtable_references: u64,
    pub transitive_allocation_symbols: u64,
    pub max_call_depth: u64,
    pub stack_bytes: Option<u64>,
}

#[derive(Debug, Error)]
pub enum ArtifactError {
    #[error("failed to read artifact {path:?}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse artifact {path:?}: {source}")]
    Object {
        path: PathBuf,
        #[source]
        source: object::Error,
    },
    #[error("artifact {path:?} has no text symbol `{symbol}`")]
    MissingSymbol { path: PathBuf, symbol: String },
    #[error("transitive analysis of `{symbol}` exceeds {limit} reachable functions")]
    TraversalLimit { symbol: String, limit: usize },
    #[error("unsupported artifact architecture {0:?}")]
    Unsupported(object::Architecture),
    #[error("failed to disassemble `{symbol}`: {source}")]
    Disassemble {
        symbol: String,
        #[source]
        source: capstone::Error,
    },
}

pub struct Analyzer {
    path: PathBuf,
    data: Vec<u8>,
}

impl Analyzer {
    pub fn open(path: &Path) -> Result<Self, ArtifactError> {
        let data = fs::read(path).map_err(|source| ArtifactError::Read {
            path: path.to_owned(),
            source,
        })?;
        object::File::parse(&*data).map_err(|source| ArtifactError::Object {
            path: path.to_owned(),
            source,
        })?;
        Ok(Self {
            path: path.to_owned(),
            data,
        })
    }

    pub fn analyze(&self, symbol: &str) -> Result<Metrics, ArtifactError> {
        let file = object::File::parse(&*self.data).map_err(|source| ArtifactError::Object {
            path: self.path.clone(),
            source,
        })?;
        let symbols = symbols(&file);
        let relocated_pointers = relocation_values(&file);
        let vtables = vtable_candidates(&file, &symbols, &relocated_pointers);
        let root = find_symbol(&symbols, symbol).ok_or_else(|| ArtifactError::MissingSymbol {
            path: self.path.clone(),
            symbol: symbol.to_owned(),
        })?;
        let disassembler = disassembler(file.architecture())?;
        let mut visited = BTreeSet::new();
        let mut metrics = Metrics::default();
        let mut pending = vec![(root.address, 0u64)];

        while let Some((address, depth)) = pending.pop() {
            if !register_function(&mut visited, address, symbol)? {
                continue;
            }
            let Some(function) = symbols.get(&address) else {
                continue;
            };
            let bytes = symbol_bytes(&file, function)?;
            let local = analyze_bytes(
                &disassembler,
                bytes,
                function.address,
                &symbols,
                &vtables,
                &relocated_pointers,
            )
            .map_err(|source| ArtifactError::Disassemble {
                symbol: function.name.clone(),
                source,
            })?;
            if address == root.address {
                metrics.text_bytes = function.size;
                metrics.instructions = local.instructions;
                metrics.branches = local.branches;
                metrics.direct_calls = local.direct_calls;
                metrics.tail_calls = local.tail_calls;
                metrics.indirect_calls = local.indirect_calls;
                metrics.loads = local.loads;
                metrics.stores = local.stores;
                metrics.panic_paths = local.panic_paths;
                metrics.vtable_references = local.vtable_references;
                metrics.allocation_symbols = local.allocation_symbols;
            }
            metrics.transitive_instructions += local.instructions;
            metrics.transitive_branches += local.branches;
            metrics.transitive_direct_calls += local.direct_calls;
            metrics.transitive_tail_calls += local.tail_calls;
            metrics.transitive_indirect_calls += local.indirect_calls;
            metrics.transitive_loads += local.loads;
            metrics.transitive_stores += local.stores;
            metrics.transitive_panic_paths += local.panic_paths;
            metrics.transitive_allocation_symbols += local.allocation_symbols;
            metrics.transitive_vtable_references += local.vtable_references;
            metrics.max_call_depth = metrics.max_call_depth.max(depth);
            for target in local.internal_calls {
                pending.push((target, depth + 1));
            }
        }
        metrics.reachable_functions = visited.len() as u64;
        Ok(metrics)
    }
}

fn register_function(
    visited: &mut BTreeSet<u64>,
    address: u64,
    symbol: &str,
) -> Result<bool, ArtifactError> {
    if visited.contains(&address) {
        return Ok(false);
    }
    if visited.len() >= MAX_REACHABLE_FUNCTIONS {
        return Err(ArtifactError::TraversalLimit {
            symbol: symbol.to_owned(),
            limit: MAX_REACHABLE_FUNCTIONS,
        });
    }
    visited.insert(address);
    Ok(true)
}

#[derive(Clone, Debug)]
struct Symbol {
    name: String,
    address: u64,
    size: u64,
    section: object::SectionIndex,
}

fn symbols(file: &object::File<'_>) -> BTreeMap<u64, Symbol> {
    let mut symbols = file
        .symbols()
        .filter(|symbol| symbol.kind() == SymbolKind::Text && symbol.address() > 0)
        .filter_map(|symbol| {
            Some(Symbol {
                name: symbol.name().ok()?.to_owned(),
                address: symbol.address(),
                size: symbol.size(),
                section: symbol.section_index()?,
            })
        })
        .collect::<Vec<_>>();
    symbols.sort_by_key(|symbol| (symbol.section.0, symbol.address));
    for index in 0..symbols.len() {
        if symbols[index].size != 0 {
            continue;
        }
        let end = symbols[index + 1..]
            .iter()
            .find(|next| {
                next.section == symbols[index].section && next.address > symbols[index].address
            })
            .map(|next| next.address)
            .or_else(|| {
                let section = file.section_by_index(symbols[index].section).ok()?;
                Some(section.address() + section.size())
            })
            .unwrap_or(symbols[index].address);
        symbols[index].size = end.saturating_sub(symbols[index].address);
    }
    symbols
        .into_iter()
        .filter(|symbol| symbol.size > 0)
        .fold(BTreeMap::new(), |mut map, symbol| {
            map.entry(symbol.address).or_insert(symbol);
            map
        })
}

fn find_symbol<'a>(symbols: &'a BTreeMap<u64, Symbol>, name: &str) -> Option<&'a Symbol> {
    symbols
        .values()
        .find(|symbol| symbol.name == name || symbol.name == format!("_{name}"))
}

fn vtable_candidates(
    file: &object::File<'_>,
    text_symbols: &BTreeMap<u64, Symbol>,
    relocated_pointers: &BTreeMap<u64, u64>,
) -> BTreeSet<u64> {
    let little_endian = file.is_little_endian();
    let mut candidates = BTreeSet::new();
    for section in file.sections() {
        if !matches!(
            section.kind(),
            SectionKind::ReadOnlyData | SectionKind::Data | SectionKind::Unknown
        ) {
            continue;
        }
        let Ok(data) = section.data() else {
            continue;
        };
        for offset in (0..data.len().saturating_sub(31)).step_by(8) {
            let words = [
                relocated_word(
                    data,
                    offset,
                    section.address(),
                    little_endian,
                    relocated_pointers,
                ),
                relocated_word(
                    data,
                    offset + 8,
                    section.address(),
                    little_endian,
                    relocated_pointers,
                ),
                relocated_word(
                    data,
                    offset + 16,
                    section.address(),
                    little_endian,
                    relocated_pointers,
                ),
                relocated_word(
                    data,
                    offset + 24,
                    section.address(),
                    little_endian,
                    relocated_pointers,
                ),
            ];
            let drop_is_valid = words[0] == 0 || text_contains(text_symbols, words[0]);
            let size_is_valid = words[1] <= (1u64 << 40);
            let align_is_valid =
                words[2] != 0 && words[2].is_power_of_two() && words[2] <= (1u64 << 20);
            if drop_is_valid
                && size_is_valid
                && align_is_valid
                && text_contains(text_symbols, words[3])
            {
                candidates.insert(section.address() + offset as u64);
            }
        }
    }
    candidates
}

fn relocation_values(file: &object::File<'_>) -> BTreeMap<u64, u64> {
    let little_endian = file.is_little_endian();
    let mut values = BTreeMap::new();

    for section in file.sections() {
        if !pointer_section(section.kind()) {
            continue;
        }
        let Ok(data) = section.data() else {
            continue;
        };
        for (offset, relocation) in section.relocations() {
            let Ok(offset) = usize::try_from(offset) else {
                continue;
            };
            let Some(raw) = data
                .get(offset..offset.saturating_add(8))
                .map(|bytes| word(bytes, little_endian))
            else {
                continue;
            };
            if let Some(value) = section_relocation_value(file, &relocation, raw) {
                values.insert(section.address() + offset as u64, value);
            }
        }
    }

    if let Some(relocations) = file.dynamic_relocations() {
        for (address, relocation) in relocations {
            let raw = raw_word_at(file, address, little_endian);
            if relocation.has_implicit_addend() && raw.is_none() {
                continue;
            }
            if let Some(value) = dynamic_relative_value(&relocation, raw.unwrap_or(0)) {
                values.insert(address, value);
            }
        }
    }

    values
}

fn section_relocation_value(
    file: &object::File<'_>,
    relocation: &Relocation,
    raw: u64,
) -> Option<u64> {
    if relocation.size() != 64 || relocation.kind() != RelocationKind::Absolute {
        return None;
    }
    let target = match relocation.target() {
        RelocationTarget::Symbol(index) => file.symbol_by_index(index).ok()?.address(),
        RelocationTarget::Section(index) => file.section_by_index(index).ok()?.address(),
        _ => return None,
    };
    let value = target.wrapping_add(relocation.addend() as u64);
    Some(if relocation.has_implicit_addend() {
        raw.wrapping_add(value)
    } else {
        value
    })
}

fn dynamic_relative_value(relocation: &Relocation, raw: u64) -> Option<u64> {
    let relative = matches!(
        relocation.flags(),
        RelocationFlags::Elf { r_type }
            if matches!(
                r_type,
                object::elf::R_X86_64_RELATIVE | object::elf::R_AARCH64_RELATIVE
            )
    );
    if !relative || relocation.target() != RelocationTarget::Absolute {
        return None;
    }
    let addend = relocation.addend() as u64;
    Some(if relocation.has_implicit_addend() {
        raw.wrapping_add(addend)
    } else {
        addend
    })
}

fn raw_word_at(file: &object::File<'_>, address: u64, little_endian: bool) -> Option<u64> {
    for section in file.sections() {
        if !pointer_section(section.kind()) {
            continue;
        }
        let Some(offset) = address.checked_sub(section.address()) else {
            continue;
        };
        let Ok(offset) = usize::try_from(offset) else {
            continue;
        };
        let Ok(data) = section.data() else {
            continue;
        };
        let Some(end) = offset.checked_add(8) else {
            continue;
        };
        if let Some(bytes) = data.get(offset..end) {
            return Some(word(bytes, little_endian));
        }
    }
    None
}

fn relocated_word(
    data: &[u8],
    offset: usize,
    section_address: u64,
    little_endian: bool,
    relocated_pointers: &BTreeMap<u64, u64>,
) -> u64 {
    relocated_pointers
        .get(&(section_address + offset as u64))
        .copied()
        .unwrap_or_else(|| word(&data[offset..offset + 8], little_endian))
}

fn pointer_section(kind: SectionKind) -> bool {
    matches!(
        kind,
        SectionKind::ReadOnlyData | SectionKind::Data | SectionKind::Unknown
    )
}

fn text_contains(symbols: &BTreeMap<u64, Symbol>, address: u64) -> bool {
    symbols
        .range(..=address)
        .next_back()
        .is_some_and(|(_, symbol)| address < symbol.address.saturating_add(symbol.size))
}

fn word(bytes: &[u8], little_endian: bool) -> u64 {
    let bytes: [u8; 8] = bytes.try_into().expect("vtable word has exact width");
    if little_endian {
        u64::from_le_bytes(bytes)
    } else {
        u64::from_be_bytes(bytes)
    }
}

fn symbol_bytes<'a>(file: &object::File<'a>, symbol: &Symbol) -> Result<&'a [u8], ArtifactError> {
    let section =
        file.section_by_index(symbol.section)
            .map_err(|source| ArtifactError::Object {
                path: PathBuf::from("<loaded artifact>"),
                source,
            })?;
    let data = section.data().map_err(|source| ArtifactError::Object {
        path: PathBuf::from("<loaded artifact>"),
        source,
    })?;
    let offset = (symbol.address - section.address()) as usize;
    Ok(&data[offset..offset + symbol.size as usize])
}

fn disassembler(architecture: object::Architecture) -> Result<Capstone, ArtifactError> {
    match architecture {
        object::Architecture::X86_64 => Capstone::new()
            .x86()
            .mode(capstone::arch::x86::ArchMode::Mode64)
            .detail(true)
            .build()
            .map_err(|_| ArtifactError::Unsupported(architecture)),
        object::Architecture::Aarch64 => Capstone::new()
            .arm64()
            .mode(capstone::arch::arm64::ArchMode::Arm)
            .detail(true)
            .build()
            .map_err(|_| ArtifactError::Unsupported(architecture)),
        _ => Err(ArtifactError::Unsupported(architecture)),
    }
}

#[derive(Default)]
struct LocalMetrics {
    instructions: u64,
    branches: u64,
    direct_calls: u64,
    tail_calls: u64,
    indirect_calls: u64,
    loads: u64,
    stores: u64,
    panic_paths: u64,
    allocation_symbols: u64,
    vtable_references: u64,
    internal_calls: Vec<u64>,
}

fn analyze_bytes(
    disassembler: &Capstone,
    bytes: &[u8],
    address: u64,
    symbols: &BTreeMap<u64, Symbol>,
    vtables: &BTreeSet<u64>,
    relocated_pointers: &BTreeMap<u64, u64>,
) -> Result<LocalMetrics, capstone::Error> {
    let instructions = disassembler.disasm_all(bytes, address)?;
    let mut metrics = LocalMetrics::default();
    let mut register_values = BTreeMap::new();
    let mut referenced_vtables = BTreeSet::new();
    for instruction in instructions.as_ref() {
        metrics.instructions += 1;
        let detail = disassembler.insn_detail(instruction)?;
        let call = detail
            .groups()
            .iter()
            .any(|group| group.0 == capstone::InsnGroupType::CS_GRP_CALL as u8);
        let branch = detail
            .groups()
            .iter()
            .any(|group| group.0 == capstone::InsnGroupType::CS_GRP_JUMP as u8);
        let tail = matches!(instruction.mnemonic(), Some("jmp" | "b" | "br"));
        if branch || tail {
            metrics.branches += 1;
        }

        let operands = detail.arch_detail().operands();
        track_vtables(
            instruction,
            &operands,
            vtables,
            relocated_pointers,
            &mut register_values,
            &mut referenced_vtables,
        );
        let direct_target = if call || tail {
            operands.iter().find_map(immediate)
        } else {
            None
        };
        if call {
            if let Some(target) = direct_target {
                metrics.direct_calls += 1;
                if let Some(target_symbol) = symbols.get(&target) {
                    if PANIC_MARKERS
                        .iter()
                        .any(|marker| target_symbol.name.contains(marker))
                    {
                        metrics.panic_paths += 1;
                    }
                    if ALLOCATION_MARKERS
                        .iter()
                        .any(|marker| target_symbol.name.contains(marker))
                    {
                        metrics.allocation_symbols += 1;
                    }
                    metrics.internal_calls.push(target);
                }
            } else {
                metrics.indirect_calls += 1;
            }
        }
        if tail {
            metrics.tail_calls += 1;
            match direct_target {
                Some(target) if target != address && symbols.contains_key(&target) => {
                    metrics.internal_calls.push(target);
                }
                None => metrics.indirect_calls += 1,
                Some(_) => {}
            }
        }

        for operand in &operands {
            if let Some(access) = memory_access(operand) {
                metrics.loads += u64::from(access.is_readable());
                metrics.stores += u64::from(access.is_writable());
            }
        }
    }
    metrics.vtable_references = referenced_vtables.len() as u64;
    Ok(metrics)
}

fn immediate(operand: &ArchOperand) -> Option<u64> {
    match operand {
        ArchOperand::X86Operand(operand) => match operand.op_type {
            X86OperandType::Imm(value) => Some(value as u64),
            _ => None,
        },
        ArchOperand::Arm64Operand(operand) => match operand.op_type {
            Arm64OperandType::Imm(value) => Some(value as u64),
            _ => None,
        },
    }
}

fn memory_access(operand: &ArchOperand) -> Option<capstone::AccessType> {
    match operand {
        ArchOperand::X86Operand(operand) => matches!(operand.op_type, X86OperandType::Mem(_))
            .then_some(operand.access)
            .flatten(),
        ArchOperand::Arm64Operand(operand) => matches!(operand.op_type, Arm64OperandType::Mem(_))
            .then_some(operand.access)
            .flatten(),
    }
}

fn track_vtables(
    instruction: &capstone::Insn<'_>,
    operands: &[ArchOperand],
    candidates: &BTreeSet<u64>,
    relocated_pointers: &BTreeMap<u64, u64>,
    registers: &mut BTreeMap<u16, u64>,
    references: &mut BTreeSet<u64>,
) {
    let mnemonic = instruction.mnemonic().unwrap_or_default();
    if matches!(mnemonic, "adr" | "adrp") {
        if let (Some(register), Some(address)) = (
            operands.first().and_then(arm_register),
            operands.get(1).and_then(arm_immediate),
        ) {
            registers.insert(register, address);
            if mnemonic == "adr" {
                record_vtable(address, candidates, relocated_pointers, references);
            }
        }
    } else if mnemonic == "add"
        && let (Some(destination), Some(source), Some(offset)) = (
            operands.first().and_then(arm_register),
            operands.get(1).and_then(arm_register),
            operands.get(2).and_then(arm_immediate),
        )
        && let Some(base) = registers.get(&source).copied()
    {
        let address = base.wrapping_add(offset);
        registers.insert(destination, address);
        record_vtable(address, candidates, relocated_pointers, references);
    } else if mnemonic == "mov"
        && let (Some(destination), Some(source)) = (
            operands.first().and_then(arm_register),
            operands.get(1).and_then(arm_register),
        )
        && let Some(value) = registers.get(&source).copied()
    {
        registers.insert(destination, value);
    }

    for operand in operands {
        match operand {
            ArchOperand::X86Operand(operand) => {
                if let X86OperandType::Mem(memory) = operand.op_type
                    && memory.base().0 == X86Reg::X86_REG_RIP as u16
                {
                    let next = instruction.address() + instruction.bytes().len() as u64;
                    record_vtable(
                        next.wrapping_add_signed(memory.disp()),
                        candidates,
                        relocated_pointers,
                        references,
                    );
                }
            }
            ArchOperand::Arm64Operand(operand) => {
                if let Arm64OperandType::Mem(memory) = operand.op_type
                    && let Some(base) = registers.get(&memory.base().0).copied()
                {
                    record_vtable(
                        base.wrapping_add_signed(i64::from(memory.disp())),
                        candidates,
                        relocated_pointers,
                        references,
                    );
                }
            }
        }
    }
}

fn arm_register(operand: &ArchOperand) -> Option<u16> {
    match operand {
        ArchOperand::Arm64Operand(operand) => match operand.op_type {
            Arm64OperandType::Reg(register) => Some(register.0),
            _ => None,
        },
        _ => None,
    }
}

fn arm_immediate(operand: &ArchOperand) -> Option<u64> {
    match operand {
        ArchOperand::Arm64Operand(operand) => match operand.op_type {
            Arm64OperandType::Imm(value) => Some(value as u64),
            _ => None,
        },
        _ => None,
    }
}

fn record_vtable(
    address: u64,
    candidates: &BTreeSet<u64>,
    relocated_pointers: &BTreeMap<u64, u64>,
    references: &mut BTreeSet<u64>,
) {
    let mut address = address;
    for _ in 0..=2 {
        if candidates.contains(&address) {
            references.insert(address);
            return;
        }
        let Some(next) = relocated_pointers.get(&address).copied() else {
            return;
        };
        if next == address {
            return;
        }
        address = next;
    }
}
