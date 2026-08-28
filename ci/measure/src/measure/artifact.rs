use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use rzpipe::{RzPipe, RzPipeSpawnOptions};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
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
}

pub struct Analyzer {
    path: PathBuf,
    pipe: RefCell<RzPipe>,
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
        Ok(Self {
            path: path.to_owned(),
            pipe: RefCell::new(pipe),
        })
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
        let root_size = root_symbol.size.filter(|size| *size != 0).or_else(|| {
            symbols
                .iter()
                .filter_map(|entry| entry.vaddr)
                .filter(|candidate| *candidate > address)
                .min()
                .map(|next| next - address)
        });
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
                    pending.push_back((callee, None));
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

        for operation in body.ops {
            let Some(offset) = operation.offset else {
                continue;
            };
            if !(start..end).contains(&offset) {
                continue;
            }
            metrics.instructions += 1;
            if branch(&operation.kind) {
                metrics.branches += 1;
            }
            let linkage = linkage(&operation, imports);
            let call = operation.kind.ends_with("call");
            if call {
                if linkage {
                    metrics.linkage_calls += 1;
                } else if operation.kind == "call" {
                    metrics.direct_calls += 1;
                } else {
                    metrics.indirect_calls += 1;
                }
                let panic = marker(&operation.disasm, PANIC_MARKERS);
                let allocation = marker(&operation.disasm, ALLOCATION_MARKERS);
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

            if unconditional_jump(&operation.kind) && !switch(&operation) {
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
    use super::{Operation, linkage, switch};
    use std::collections::BTreeSet;

    #[test]
    fn distinguishes_linker_relocations_from_dynamic_dispatch() {
        let linker_call = Operation {
            offset: Some(0x1000),
            kind: "ircall".to_owned(),
            disasm: "call qword [reloc.memcpy]".to_owned(),
            jump: None,
            ptr: Some(0x2000),
            flags: Vec::new(),
        };
        let dynamic_call = Operation {
            offset: Some(0x1008),
            kind: "rcall".to_owned(),
            disasm: "call rax".to_owned(),
            jump: None,
            ptr: None,
            flags: Vec::new(),
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
        };
        assert!(switch(&jump_table));
    }

    #[test]
    fn ignores_negative_rizin_pointer_sentinels() {
        let operation: Operation =
            serde_json::from_str(r#"{"type":"ircall","jump":-2063541689}"#).unwrap();
        assert_eq!(operation.jump, None);
    }
}
