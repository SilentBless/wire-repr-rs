mod linkage;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::fs;
use std::ops::Bound::{Excluded, Unbounded};
use std::path::{Path, PathBuf};

use rzpipe::{RzPipe, RzPipeSpawnOptions};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use linkage::CachedLinkage;
use thiserror::Error;

const ALLOCATION_MARKERS: &[&str] = &[
    "__rust_alloc",
    "__rust_realloc",
    "__rust_dealloc",
    "exchange_malloc",
    "::alloc::alloc",
    "raw_vec",
    "RawVec",
    "malloc",
    "realloc",
    "HeapAlloc",
];
const PANIC_MARKERS: &[&str] = &[
    "panic",
    "bounds_check",
    "slice_index",
    "copy_from_slice",
    "assert_failed",
    "unwrap_failed",
    "expect_failed",
];
const MAX_REACHABLE_FUNCTIONS: usize = 256;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Metrics {
    pub text_bytes: u64,
    pub instructions: u64,
    pub branches: u64,
    pub direct_calls: u64,
    pub linkage_calls: u64,
    pub tail_calls: u64,
    pub indirect_calls: u64,
    pub panic_paths: u64,
    pub allocation_symbols: u64,
    pub reachable_functions: u64,
    pub transitive_instructions: u64,
    pub transitive_branches: u64,
    pub transitive_direct_calls: u64,
    pub transitive_linkage_calls: u64,
    pub transitive_tail_calls: u64,
    pub transitive_indirect_calls: u64,
    pub transitive_panic_paths: u64,
    pub transitive_allocation_symbols: u64,
    pub max_call_depth: u64,
    pub stack_bytes: Option<u64>,
}

#[derive(Debug, Error)]
pub enum ArtifactError {
    #[error("failed to access artifact {path:?}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("artifact path {0:?} is not valid UTF-8")]
    Path(PathBuf),
    #[error("invalid artifact symbol `{0}`")]
    InvalidSymbol(String),
    #[error("failed to start Rizin for {path:?}: {detail}")]
    Start { path: PathBuf, detail: String },
    #[error("Rizin command `{command}` failed: {detail}")]
    Command { command: String, detail: String },
    #[error("Rizin returned invalid JSON for `{command}`: {source}")]
    Json {
        command: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("artifact {path:?} has no text symbol `{symbol}`")]
    MissingSymbol { path: PathBuf, symbol: String },
    #[error("transitive analysis of `{symbol}` exceeds {limit} reachable functions")]
    TraversalLimit { symbol: String, limit: usize },
    #[error(
        "callee at {address:#x} has declared size {declared_size}, crossing linked symbol at {next_symbol:#x}"
    )]
    ConflictingSymbolBounds {
        address: u64,
        declared_size: u64,
        next_symbol: u64,
    },
}

pub struct Analyzer {
    path: PathBuf,
    pipe: RefCell<RzPipe>,
    x86_64: bool,
}

impl Analyzer {
    pub fn open(path: &Path) -> Result<Self, ArtifactError> {
        fs::metadata(path).map_err(|source| ArtifactError::Read {
            path: path.to_owned(),
            source,
        })?;
        let artifact = path
            .to_str()
            .ok_or_else(|| ArtifactError::Path(path.to_owned()))?;
        let executable = env::var("WIRE_REPR_RIZIN").unwrap_or_else(|_| "rizin".to_owned());
        let options = RzPipeSpawnOptions {
            exepath: executable,
            args: vec!["-2"],
        };
        let mut pipe =
            RzPipe::spawn(artifact, Some(options)).map_err(|source| ArtifactError::Start {
                path: path.to_owned(),
                detail: source.to_string(),
            })?;
        command(&mut pipe, "e scr.color=0")?;
        let pdb = path.with_extension("pdb");
        if pdb.is_file() {
            command(&mut pipe, &format!("idp {}", command_path(&pdb)))?;
        }
        let mut analyzer = Self {
            path: path.to_owned(),
            pipe: RefCell::new(pipe),
            x86_64: false,
        };
        let information: BinaryInfo = analyzer.json("ij")?;
        analyzer.x86_64 = information.bin.arch == "x86" && information.bin.bits == 64;
        Ok(analyzer)
    }

    pub fn analyze(&self, symbol: &str) -> Result<Metrics, ArtifactError> {
        if !identifier(symbol) {
            return Err(ArtifactError::InvalidSymbol(symbol.to_owned()));
        }
        let symbols: Vec<SymbolInfo> = self.json("isj")?;
        let root_symbol = symbols
            .iter()
            .find(|entry| entry.name == symbol)
            .or_else(|| {
                symbols
                    .iter()
                    .find(|entry| entry.name.strip_prefix('_') == Some(symbol))
            })
            .ok_or_else(|| ArtifactError::MissingSymbol {
                path: self.path.clone(),
                symbol: symbol.to_owned(),
            })?;
        let address = root_symbol
            .vaddr
            .ok_or_else(|| ArtifactError::MissingSymbol {
                path: self.path.clone(),
                symbol: symbol.to_owned(),
            })?;
        let symbol_sizes = linked_symbol_sizes(&symbols);
        let root_size = linked_symbol_size(address, root_symbol.size, &symbol_sizes)?;
        let imports: Vec<ImportInfo> = self.json("iij")?;
        let import_addresses = imports
            .iter()
            .filter_map(|import| import.plt)
            .collect::<BTreeSet<_>>();

        let mut functions = BTreeMap::new();
        let mut pending = VecDeque::from([(address, root_size)]);
        while let Some((current, exact_size)) = pending.pop_front() {
            if functions.contains_key(&current) {
                continue;
            }
            if functions.len() == MAX_REACHABLE_FUNCTIONS {
                return Err(ArtifactError::TraversalLimit {
                    symbol: symbol.to_owned(),
                    limit: MAX_REACHABLE_FUNCTIONS,
                });
            }
            let local = self.analyze_function(current, exact_size, &import_addresses)?;
            for &callee in &local.callees {
                if !functions.contains_key(&callee) {
                    pending.push_back((callee, linked_symbol_size(callee, None, &symbol_sizes)?));
                }
            }
            functions.insert(current, local);
        }

        let root = functions
            .get(&address)
            .expect("root function is inserted before aggregation");
        let mut metrics = Metrics {
            text_bytes: root.text_bytes,
            instructions: root.instructions,
            branches: root.branches,
            direct_calls: root.direct_calls,
            linkage_calls: root.linkage_calls,
            tail_calls: root.tail_calls,
            indirect_calls: root.indirect_calls,
            panic_paths: root.panic_paths,
            allocation_symbols: root.allocation_symbols,
            reachable_functions: functions.len() as u64,
            max_call_depth: max_call_depth(address, &functions),
            stack_bytes: Some(root.stack_bytes),
            ..Metrics::default()
        };
        for local in functions.values() {
            metrics.transitive_instructions += local.instructions;
            metrics.transitive_branches += local.branches;
            metrics.transitive_direct_calls += local.direct_calls;
            metrics.transitive_linkage_calls += local.linkage_calls;
            metrics.transitive_tail_calls += local.tail_calls;
            metrics.transitive_indirect_calls += local.indirect_calls;
            metrics.transitive_panic_paths += local.panic_paths;
            metrics.transitive_allocation_symbols += local.allocation_symbols;
        }
        Ok(metrics)
    }

    fn analyze_function(
        &self,
        address: u64,
        exact_size: Option<u64>,
        imports: &BTreeSet<u64>,
    ) -> Result<LocalMetrics, ArtifactError> {
        let seek = format!("0x{address:x}");
        self.run(&format!("afr @ {seek}"))?;
        let mut information: Vec<FunctionInfo> = self.json(&format!("afij @ {seek}"))?;
        let information = information
            .iter()
            .position(|function| function.offset == address)
            .map(|index| information.swap_remove(index))
            .or_else(|| information.into_iter().next())
            .ok_or_else(|| ArtifactError::MissingSymbol {
                path: self.path.clone(),
                symbol: seek.clone(),
            })?;
        let body: FunctionBody = self.json(&format!("pdfj @ {seek}"))?;
        let blocks: Vec<BlockInfo> = self.json(&format!("afbj @ {seek}"))?;
        let block_starts: BTreeSet<_> = blocks.into_iter().map(|block| block.addr).collect();
        let mut cached = CachedLinkage::default();
        let start = exact_size.map_or(information.offset, |_| address);
        let end = exact_size.map_or_else(
            || {
                information
                    .maxbound
                    .filter(|bound| *bound >= start)
                    .unwrap_or_else(|| start.saturating_add(information.size))
            },
            |size| start.saturating_add(size),
        );
        let mut metrics = LocalMetrics {
            text_bytes: end.saturating_sub(start),
            stack_bytes: information.stackframe,
            ..LocalMetrics::default()
        };

        for operation in &body.ops {
            let Some(offset) = operation.offset else {
                continue;
            };
            if !(start..end).contains(&offset) {
                continue;
            }
            if block_starts.contains(&offset) {
                cached.clear();
            }
            metrics.instructions += 1;
            if branch(&operation.kind) {
                metrics.branches += 1;
            }
            let direct_linkage = linkage(operation, imports);
            let cached_source = cached.source(operation);
            let linkage = direct_linkage || cached_source.is_some();
            let call = operation.kind.ends_with("call");
            if call {
                if linkage {
                    metrics.linkage_calls += 1;
                } else if operation.kind == "call" {
                    metrics.direct_calls += 1;
                } else {
                    metrics.indirect_calls += 1;
                }
                let target = cached_source.unwrap_or(&operation.disasm);
                let panic = marker(target, PANIC_MARKERS);
                let allocation = marker(target, ALLOCATION_MARKERS);
                metrics.panic_paths += u64::from(panic);
                metrics.allocation_symbols += u64::from(allocation);
                if operation.kind == "call"
                    && !linkage
                    && !panic
                    && !allocation
                    && let Some(target) = operation.jump
                    && !imports.contains(&target)
                    && !(start..end).contains(&target)
                {
                    metrics.callees.insert(target);
                }
            }

            if unconditional_jump(&operation.kind) && !switch(operation) {
                let leaves_function = operation
                    .jump
                    .is_none_or(|target| !(start..end).contains(&target));
                if leaves_function {
                    metrics.tail_calls += 1;
                    if linkage {
                        metrics.linkage_calls += 1;
                    } else if operation.kind != "jmp" {
                        metrics.indirect_calls += 1;
                    } else if let Some(target) = operation.jump
                        && !imports.contains(&target)
                    {
                        metrics.callees.insert(target);
                    }
                }
            }
            if self.x86_64 && !block_starts.is_empty() && !branch(&operation.kind) {
                cached.observe(operation, direct_linkage);
            } else {
                cached.clear();
            }
        }
        Ok(metrics)
    }

    fn run(&self, command_text: &str) -> Result<String, ArtifactError> {
        command(&mut self.pipe.borrow_mut(), command_text)
    }

    fn json<T: DeserializeOwned>(&self, command_text: &str) -> Result<T, ArtifactError> {
        let value = self
            .pipe
            .borrow_mut()
            .cmdj(command_text)
            .map_err(|source| ArtifactError::Command {
                command: command_text.to_owned(),
                detail: source.to_string(),
            })?;
        serde_json::from_value(value).map_err(|source| ArtifactError::Json {
            command: command_text.to_owned(),
            source,
        })
    }
}

impl Drop for Analyzer {
    fn drop(&mut self) {
        self.pipe.get_mut().close();
    }
}

#[derive(Default)]
struct LocalMetrics {
    text_bytes: u64,
    instructions: u64,
    branches: u64,
    direct_calls: u64,
    linkage_calls: u64,
    tail_calls: u64,
    indirect_calls: u64,
    panic_paths: u64,
    allocation_symbols: u64,
    stack_bytes: u64,
    callees: BTreeSet<u64>,
}

#[derive(Deserialize)]
struct BinaryInfo {
    bin: ArchitectureInfo,
}

#[derive(Deserialize)]
struct ArchitectureInfo {
    #[serde(default)]
    arch: String,
    #[serde(default)]
    bits: u64,
}

#[derive(Deserialize)]
struct BlockInfo {
    addr: u64,
}

#[derive(Deserialize)]
struct SymbolInfo {
    name: String,
    vaddr: Option<u64>,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Deserialize)]
struct ImportInfo {
    #[serde(default)]
    plt: Option<u64>,
}

#[derive(Deserialize)]
struct FunctionInfo {
    offset: u64,
    size: u64,
    #[serde(default)]
    maxbound: Option<u64>,
    #[serde(default)]
    stackframe: u64,
}

#[derive(Deserialize)]
struct FunctionBody {
    ops: Vec<Operation>,
}

#[derive(Deserialize)]
struct Operation {
    #[serde(default, deserialize_with = "nonnegative_u64")]
    offset: Option<u64>,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    disasm: String,
    #[serde(default)]
    esil: String,
    #[serde(default, deserialize_with = "nonnegative_u64")]
    jump: Option<u64>,
    #[serde(default, deserialize_with = "nonnegative_u64")]
    ptr: Option<u64>,
    #[serde(default)]
    flags: Vec<String>,
}

fn command(pipe: &mut RzPipe, command_text: &str) -> Result<String, ArtifactError> {
    pipe.cmd(command_text)
        .map_err(|source| ArtifactError::Command {
            command: command_text.to_owned(),
            detail: source.to_string(),
        })
}

fn linked_symbol_sizes(symbols: &[SymbolInfo]) -> BTreeMap<u64, u64> {
    let mut sizes = BTreeMap::new();
    for symbol in symbols {
        if let Some(address) = symbol.vaddr {
            let size = sizes.entry(address).or_insert(0);
            *size = (*size).max(symbol.size.unwrap_or(0));
        }
    }
    sizes
}

fn linked_symbol_size(
    address: u64,
    requested_size: Option<u64>,
    symbols: &BTreeMap<u64, u64>,
) -> Result<Option<u64>, ArtifactError> {
    let Some(size) = requested_size
        .filter(|&size| size != 0)
        .or_else(|| symbols.get(&address).copied())
    else {
        return Ok(None);
    };
    let next_symbol = symbols
        .range((Excluded(address), Unbounded))
        .next()
        .map(|(&next, _)| next);
    if size != 0 {
        if let Some(next_symbol) = next_symbol
            && size > next_symbol - address
        {
            return Err(ArtifactError::ConflictingSymbolBounds {
                address,
                declared_size: size,
                next_symbol,
            });
        }
        return Ok(Some(size));
    }
    Ok(next_symbol.map(|next| next - address))
}

fn max_call_depth(root: u64, functions: &BTreeMap<u64, LocalMetrics>) -> u64 {
    fn visit(
        address: u64,
        functions: &BTreeMap<u64, LocalMetrics>,
        active: &mut BTreeSet<u64>,
        memo: &mut BTreeMap<u64, u64>,
    ) -> Option<u64> {
        if let Some(depth) = memo.get(&address) {
            return Some(*depth);
        }
        if !active.insert(address) {
            return None;
        }
        let depth = functions
            .get(&address)
            .into_iter()
            .flat_map(|function| &function.callees)
            .filter_map(|callee| {
                functions
                    .contains_key(callee)
                    .then(|| visit(*callee, functions, active, memo))
                    .flatten()
                    .map(|depth| depth + 1)
            })
            .max()
            .unwrap_or(0);
        active.remove(&address);
        memo.insert(address, depth);
        Some(depth)
    }

    visit(root, functions, &mut BTreeSet::new(), &mut BTreeMap::new()).unwrap_or(0)
}

fn identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn command_path(path: &Path) -> String {
    let path = path
        .to_string_lossy()
        .replace('\\', "/")
        .replace('"', "\\\"");
    format!("\"{path}\"")
}

fn branch(kind: &str) -> bool {
    kind.ends_with("jmp") || kind == "switch"
}

fn unconditional_jump(kind: &str) -> bool {
    matches!(kind, "jmp" | "rjmp" | "ijmp" | "ujmp")
}

#[derive(Deserialize)]
#[serde(untagged)]
enum JsonInteger {
    Unsigned(u64),
    Signed(i64),
}

fn nonnegative_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match Option::<JsonInteger>::deserialize(deserializer)? {
        Some(JsonInteger::Unsigned(value)) => Some(value),
        Some(JsonInteger::Signed(value)) => u64::try_from(value).ok(),
        None => None,
    })
}

fn switch(operation: &Operation) -> bool {
    operation
        .flags
        .iter()
        .any(|flag| flag.starts_with("switch."))
}

fn linkage(operation: &Operation, imports: &BTreeSet<u64>) -> bool {
    operation.disasm.contains("reloc.")
        || operation.disasm.contains("sym.imp.")
        || operation
            .jump
            .into_iter()
            .chain(operation.ptr)
            .any(|target| imports.contains(&target))
}

fn marker(value: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| value.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::{Operation, SymbolInfo, linkage, linked_symbol_size, linked_symbol_sizes, switch};
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn distinguishes_linker_relocations_from_dynamic_dispatch() {
        let linker_call = Operation {
            offset: Some(0x1000),
            kind: "ircall".to_owned(),
            disasm: "call qword [reloc.memcpy]".to_owned(),
            jump: None,
            ptr: Some(0x2000),
            flags: Vec::new(),
            esil: String::new(),
        };
        let dynamic_call = Operation {
            offset: Some(0x1008),
            kind: "rcall".to_owned(),
            disasm: "call rax".to_owned(),
            jump: None,
            ptr: None,
            flags: Vec::new(),
            esil: String::new(),
        };

        assert!(linkage(&linker_call, &BTreeSet::new()));
        assert!(!linkage(&dynamic_call, &BTreeSet::new()));
        let jump_table = Operation {
            offset: Some(0x1010),
            kind: "rjmp".to_owned(),
            disasm: "jmp rsi".to_owned(),
            jump: None,
            ptr: None,
            flags: vec!["switch.0x1000".to_owned()],
            esil: String::new(),
        };
        assert!(switch(&jump_table));
    }

    #[test]
    fn linked_callee_bounds_exclude_the_following_function() {
        let symbols = BTreeMap::from([(0x100000dd8, 0), (0x100001734, 0), (0x1000042bc, 16)]);
        let start = 0x100000dd8;
        let end = start
            + linked_symbol_size(start, Some(0), &symbols)
                .unwrap()
                .unwrap();
        assert!((start..end).contains(&0x100001730));
        assert!(!(start..end).contains(&0x100001754));
        assert_eq!(
            linked_symbol_size(0x1000042bc, None, &symbols).unwrap(),
            Some(16)
        );
        assert_eq!(
            linked_symbol_size(0x100001000, None, &symbols).unwrap(),
            None
        );
        assert_eq!(
            linked_symbol_size(0x1000, None, &BTreeMap::from([(0x1000, 0)])).unwrap(),
            None
        );
    }

    #[test]
    fn requested_root_bounds_preserve_late_calls_without_including_larger_aliases() {
        let symbols: Vec<SymbolInfo> = serde_json::from_str(
            r#"[
                {"name":"short_alias","vaddr":4096,"size":4},
                {"name":"requested_root","vaddr":4096,"size":32},
                {"name":"long_alias","vaddr":4096,"size":64},
                {"name":"following_function","vaddr":4160,"size":16}
            ]"#,
        )
        .unwrap();
        let root = &symbols[1];
        let start = root.vaddr.unwrap();
        let sizes = linked_symbol_sizes(&symbols);
        let end = start
            + linked_symbol_size(start, root.size, &sizes)
                .unwrap()
                .unwrap();
        assert!((start..end).contains(&0x101c));
        assert!(!(start..end).contains(&0x1024));
        let callee_end = start + linked_symbol_size(start, None, &sizes).unwrap().unwrap();
        assert!((start..callee_end).contains(&0x103c));
        assert!(!(start..callee_end).contains(&0x1040));
    }

    #[test]
    fn conflicting_alias_bounds_cannot_include_the_following_function() {
        let symbols = BTreeMap::from([(0x1000, 128), (0x1040, 16)]);
        assert!(matches!(
            linked_symbol_size(0x1000, None, &symbols),
            Err(super::ArtifactError::ConflictingSymbolBounds {
                address: 0x1000,
                declared_size: 128,
                next_symbol: 0x1040,
            })
        ));
    }

    #[test]
    fn conflicting_root_bounds_cannot_include_the_following_function() {
        let symbols = BTreeMap::from([(0x1000, 128), (0x1040, 16)]);
        assert!(matches!(
            linked_symbol_size(0x1000, Some(128), &symbols),
            Err(super::ArtifactError::ConflictingSymbolBounds {
                address: 0x1000,
                declared_size: 128,
                next_symbol: 0x1040,
            })
        ));
    }

    #[test]
    fn ignores_negative_rizin_pointer_sentinels() {
        let operation: Operation =
            serde_json::from_str(r#"{"type":"ircall","jump":-2063541689}"#).unwrap();
        assert_eq!(operation.jump, None);
    }
}
