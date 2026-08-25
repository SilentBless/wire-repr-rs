use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::{Probe, Workload, WorkloadError};

#[derive(Clone, Debug)]
pub struct Discovered {
    pub manifest: PathBuf,
    pub root: PathBuf,
    pub config: Workload,
    pub roles: BTreeMap<String, PathBuf>,
    pub probes: Vec<ResolvedProbe>,
}

#[derive(Clone, Debug)]
pub struct ResolvedProbe {
    pub config: Probe,
    pub source: PathBuf,
}

pub fn discover(root: &Path) -> Result<Vec<Discovered>, WorkloadError> {
    let root = canonical(root)?;
    let mut manifests = Vec::new();
    collect(&root, &mut manifests)?;
    manifests.sort();
    if manifests.is_empty() {
        return Err(WorkloadError::Invalid {
            path: root,
            message: "no workload.toml files found".to_owned(),
        });
    }

    let mut names = BTreeSet::new();
    let mut workloads = Vec::with_capacity(manifests.len());
    for manifest in manifests {
        let source = fs::read_to_string(&manifest).map_err(|source| WorkloadError::Io {
            path: manifest.clone(),
            source,
        })?;
        let config = Workload::parse(&manifest, &source)?;
        if !names.insert(config.name.clone()) {
            return Err(WorkloadError::Invalid {
                path: manifest,
                message: format!("duplicate workload name `{}`", config.name),
            });
        }
        let workload_root = manifest
            .parent()
            .expect("workload manifest has a parent")
            .to_owned();
        let relative = workload_root
            .strip_prefix(&root)
            .expect("collected workload stays below discovery root")
            .iter()
            .map(|part| part.to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if config.name != relative {
            return Err(WorkloadError::Invalid {
                path: manifest,
                message: format!(
                    "name `{}` must match its zone path `{relative}`",
                    config.name
                ),
            });
        }

        let canonical_root = canonical(&workload_root)?;
        let mut roles = BTreeMap::new();
        for (role, relative_source) in &config.roles {
            let source = canonical(&workload_root.join(relative_source))?;
            if !source.starts_with(&canonical_root) {
                return Err(WorkloadError::Invalid {
                    path: source,
                    message: format!("role `{role}` escapes its workload zone"),
                });
            }
            roles.insert(role.clone(), source);
        }
        let mut probes = Vec::with_capacity(config.probes.len());
        for probe in &config.probes {
            let source = canonical(&workload_root.join(&probe.source))?;
            if !source.starts_with(&canonical_root) {
                return Err(WorkloadError::Invalid {
                    path: source,
                    message: format!("probe `{}` escapes its workload zone", probe.name),
                });
            }
            probes.push(ResolvedProbe {
                config: probe.clone(),
                source,
            });
        }
        workloads.push(Discovered {
            manifest,
            root: workload_root,
            config,
            roles,
            probes,
        });
    }
    Ok(workloads)
}

fn collect(directory: &Path, manifests: &mut Vec<PathBuf>) -> Result<(), WorkloadError> {
    let mut entries = fs::read_dir(directory)
        .map_err(|source| WorkloadError::Io {
            path: directory.to_owned(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| WorkloadError::Io {
            path: directory.to_owned(),
            source,
        })?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| WorkloadError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            collect(&path, manifests)?;
        } else if file_type.is_file() && entry.file_name() == "workload.toml" {
            manifests.push(path);
        }
    }
    Ok(())
}

fn canonical(path: &Path) -> Result<PathBuf, WorkloadError> {
    path.canonicalize().map_err(|source| WorkloadError::Io {
        path: path.to_owned(),
        source,
    })
}
