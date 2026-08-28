use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
mod discovery;

pub use discovery::{Discovered, ResolvedProbe, discover};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WorkloadError {
    #[error("failed to parse {path:?}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("invalid workload {path:?}: {message}")]
    Invalid { path: PathBuf, message: String },
    #[error("failed to access workload path {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Workload {
    pub name: String,
    pub roles: BTreeMap<String, PathBuf>,
    #[serde(default)]
    pub probes: Vec<Probe>,
    pub cases: Vec<Case>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    pub name: String,
    pub role: String,
    pub source: PathBuf,
    pub entry: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub name: String,
    pub entry: String,
    pub seeds: Vec<i64>,
    #[serde(default)]
    pub runtime: Runtime,
    #[serde(default)]
    pub formulas: Vec<Formula>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    #[serde(default = "default_samples")]
    pub samples: usize,
    #[serde(default = "default_target_ms")]
    pub target_ms: u64,
    #[serde(default = "default_warmup")]
    pub warmup: u64,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            samples: default_samples(),
            target_ms: default_target_ms(),
            warmup: default_warmup(),
        }
    }
}

const fn default_samples() -> usize {
    21
}

const fn default_target_ms() -> u64 {
    20
}

const fn default_warmup() -> u64 {
    1_000
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Formula {
    pub name: String,
    pub expression: String,
    #[serde(default)]
    pub unit: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub name: String,
    pub level: Level,
    #[serde(rename = "assert")]
    pub assertion: String,
    #[serde(default)]
    pub message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Error,
    Attention,
    Info,
}

impl Workload {
    pub fn parse(path: &Path, source: &str) -> Result<Self, WorkloadError> {
        let workload: Self = toml::from_str(source).map_err(|source| WorkloadError::Parse {
            path: path.to_owned(),
            source,
        })?;
        workload.validate(path)?;
        Ok(workload)
    }

    pub fn validate(&self, path: &Path) -> Result<(), WorkloadError> {
        let invalid = |message: String| WorkloadError::Invalid {
            path: path.to_owned(),
            message,
        };

        if !zone_name(&self.name) {
            return Err(invalid(
                "name must contain only slash-separated ASCII letters, digits, `_`, or `-`"
                    .to_owned(),
            ));
        }
        if !self.roles.contains_key("generated") || !self.roles.contains_key("idiomatic") {
            return Err(invalid(
                "roles must define `generated` and `idiomatic`".to_owned(),
            ));
        }
        if !self.roles.keys().any(|role| role.starts_with("best_")) {
            return Err(invalid("at least one `best_*` role is required".to_owned()));
        }
        for (role, source) in &self.roles {
            if !identifier(role) {
                return Err(invalid(format!("role `{role}` is not an identifier")));
            }
            if !relative_rust(source) {
                return Err(invalid(format!(
                    "role `{role}` must name a relative Rust source below the workload"
                )));
            }
            if source.starts_with(Path::new("metrics")) {
                return Err(invalid(format!(
                    "role `{role}` cannot use a source from the metrics zone"
                )));
            }
        }
        let mut probe_names = BTreeSet::new();
        for probe in &self.probes {
            if !identifier(&probe.name) || !probe_names.insert(&probe.name) {
                return Err(invalid(format!(
                    "probe name `{}` must be a unique identifier",
                    probe.name
                )));
            }
            if !self.roles.contains_key(&probe.role) {
                return Err(invalid(format!(
                    "probe `{}` references unknown role `{}`",
                    probe.name, probe.role
                )));
            }
            if !identifier(&probe.entry)
                || !relative_rust(&probe.source)
                || !probe.source.starts_with(Path::new("metrics"))
            {
                return Err(invalid(format!(
                    "probe `{}` must name an entry and Rust source inside metrics/",
                    probe.name
                )));
            }
        }

        if self.cases.is_empty() {
            return Err(invalid("at least one case is required".to_owned()));
        }
        let mut case_names = BTreeSet::new();
        for case in &self.cases {
            if !case_names.insert(&case.name) {
                return Err(invalid(format!("duplicate case `{}`", case.name)));
            }
            if !identifier(&case.name) || !identifier(&case.entry) {
                return Err(invalid(format!(
                    "case `{}` name and entry must be identifiers",
                    case.name
                )));
            }
            if case.seeds.is_empty() {
                return Err(invalid(format!("case `{}` has no seeds", case.name)));
            }
            if case.runtime.samples == 0 || case.runtime.samples.is_multiple_of(2) {
                return Err(invalid(format!(
                    "case `{}` runtime samples must be a positive odd number",
                    case.name
                )));
            }
            if case.runtime.target_ms == 0 {
                return Err(invalid(format!(
                    "case `{}` runtime target_ms must be positive",
                    case.name
                )));
            }
            unique_names(
                case.formulas.iter().map(|formula| formula.name.as_str()),
                "formula",
                &case.name,
                path,
            )?;
            unique_names(
                case.rules.iter().map(|rule| rule.name.as_str()),
                "rule",
                &case.name,
                path,
            )?;
        }
        Ok(())
    }
}
fn zone_name(value: &str) -> bool {
    !value.is_empty()
        && value.split('/').all(|component| {
            let mut bytes = component.bytes();
            matches!(
                bytes.next(),
                Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
            ) && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
}

fn unique_names<'a>(
    names: impl Iterator<Item = &'a str>,
    owner: &str,
    case: &str,
    path: &Path,
) -> Result<(), WorkloadError> {
    let mut used = BTreeSet::new();
    for name in names {
        if name.is_empty() || !used.insert(name) {
            return Err(WorkloadError::Invalid {
                path: path.to_owned(),
                message: format!("case `{case}` has an empty or duplicate {owner} `{name}`"),
            });
        }
    }
    Ok(())
}

fn relative_rust(source: &Path) -> bool {
    source.extension().and_then(|value| value.to_str()) == Some("rs")
        && !source.is_absolute()
        && !source.components().any(|part| part == Component::ParentDir)
}

fn identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
