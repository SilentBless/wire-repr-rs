use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HarnessError {
    #[error("failed to access harness path {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("harness build failed: {0}")]
    Build(String),
    #[error("harness command failed: {0}")]
    Command(String),
    #[error("invalid harness output: {0}")]
    Output(String),
}

#[derive(Clone, Debug)]
pub struct HarnessBuilder {
    workspace: PathBuf,
    target: PathBuf,
    toolchain: String,
}

impl HarnessBuilder {
    pub fn new(workspace: &Path, target: PathBuf, toolchain: impl Into<String>) -> Self {
        Self {
            workspace: workspace.to_owned(),
            target,
            toolchain: toolchain.into(),
        }
    }

    pub fn build(&self, role: &Path, entry: &str) -> Result<Harness, HarnessError> {
        let role = canonical(role)?;
        if !identifier(entry) {
            return Err(HarnessError::Build(format!(
                "entry `{entry}` is not a Rust identifier"
            )));
        }
        let mut hasher = DefaultHasher::new();
        role.hash(&mut hasher);
        entry.hash(&mut hasher);
        let id = format!("{:016x}", hasher.finish());
        let root = self.target.join("harnesses").join(&id);
        let source_dir = root.join("src");
        fs::create_dir_all(&source_dir).map_err(|source| HarnessError::Io {
            path: source_dir.clone(),
            source,
        })?;
        let package = format!("wire-repr-measure-{id}");
        let wire_repr = self.workspace.join("wire-repr");
        let manifest = format!(
            r#"[package]
name = "{package}"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
wire-repr = {{ path = {} }}
thiserror = "2"

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = false

[workspace]
"#,
            serde_json::to_string(&wire_repr.to_string_lossy()).expect("path serializes")
        );
        write(&root.join("Cargo.toml"), &manifest)?;
        write(&source_dir.join("main.rs"), &main_source(&role, entry))?;

        let assembly = root.join("measure.s");
        let build_target = self.target.join("build");
        let emit = format!("--emit=link,asm={}", assembly.display());
        let output = Command::new("cargo")
            .arg(format!("+{}", self.toolchain))
            .args([
                "rustc",
                "--release",
                "--manifest-path",
                &root.join("Cargo.toml").to_string_lossy(),
                "--message-format=json-render-diagnostics",
                "--",
                &emit,
                "-C",
                "link-dead-code=yes",
            ])
            .env("CARGO_TARGET_DIR", &build_target)
            .output()
            .map_err(|source| HarnessError::Io {
                path: root.join("Cargo.toml"),
                source,
            })?;
        if !output.status.success() {
            return Err(HarnessError::Build(cargo_failure(&output)));
        }
        let executable = output
            .stdout
            .split(|byte| *byte == b'\n')
            .filter_map(|line| serde_json::from_slice::<CargoMessage>(line).ok())
            .filter_map(|message| message.executable)
            .next_back()
            .ok_or_else(|| HarnessError::Build("cargo did not report an executable".to_owned()))?;
        if !assembly.is_file() {
            return Err(HarnessError::Build(format!(
                "rustc did not emit {}",
                assembly.display()
            )));
        }
        Ok(Harness {
            executable,
            assembly,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Harness {
    executable: PathBuf,
    assembly: PathBuf,
}

impl Harness {
    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    #[must_use]
    pub fn assembly(&self) -> &Path {
        &self.assembly
    }

    pub fn check(&self, seeds: &[i64]) -> Result<Vec<(i64, u64)>, HarnessError> {
        let mut arguments = vec!["check".to_owned()];
        arguments.extend(seeds.iter().map(ToString::to_string));
        let output = self.run(&arguments)?;
        output
            .lines()
            .map(|line| {
                let mut fields = line.split_whitespace();
                if fields.next() != Some("result") {
                    return Err(HarnessError::Output(line.to_owned()));
                }
                let seed = parse(fields.next(), line)?;
                let value = parse(fields.next(), line)?;
                if fields.next().is_some() {
                    return Err(HarnessError::Output(line.to_owned()));
                }
                Ok((seed, value))
            })
            .collect()
    }

    pub fn sample(&self, seed: i64, warmup: u64, iterations: u64) -> Result<Sample, HarnessError> {
        let output = self.run(&[
            "sample".to_owned(),
            seed.to_string(),
            warmup.to_string(),
            iterations.to_string(),
        ])?;
        let line = output.trim();
        let mut fields = line.split_whitespace();
        if fields.next() != Some("sample") {
            return Err(HarnessError::Output(line.to_owned()));
        }
        let elapsed_ns: u128 = parse(fields.next(), line)?;
        let reported_iterations: u64 = parse(fields.next(), line)?;
        let digest: u64 = parse(fields.next(), line)?;
        if fields.next().is_some() || reported_iterations != iterations {
            return Err(HarnessError::Output(line.to_owned()));
        }
        Ok(Sample {
            elapsed_ns,
            iterations,
            digest,
        })
    }

    fn run(&self, arguments: &[String]) -> Result<String, HarnessError> {
        let output = Command::new(&self.executable)
            .args(arguments)
            .output()
            .map_err(|source| HarnessError::Io {
                path: self.executable.clone(),
                source,
            })?;
        if !output.status.success() {
            return Err(HarnessError::Command(command_failure(&output)));
        }
        String::from_utf8(output.stdout).map_err(|error| HarnessError::Output(error.to_string()))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub elapsed_ns: u128,
    pub iterations: u64,
    pub digest: u64,
}

#[derive(Deserialize)]
struct CargoMessage {
    executable: Option<PathBuf>,
    message: Option<CargoDiagnostic>,
}

#[derive(Deserialize)]
struct CargoDiagnostic {
    rendered: Option<String>,
}

fn main_source(role: &Path, entry: &str) -> String {
    let role = format!("{:?}", role.to_string_lossy());
    format!(
        r###"#![allow(unsafe_code)]
#[path = {role}]
mod implementation;

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn measure_entry(seed: u64) -> u64 {{
    implementation::{entry}(seed)
}}

fn main() {{
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {{
        Some("check") => {{
            for seed in arguments {{
                let signed: i64 = seed.parse().unwrap();
                println!("result {{signed}} {{}}", measure_entry(signed as u64));
            }}
        }}
        Some("sample") => {{
            let signed: i64 = arguments.next().unwrap().parse().unwrap();
            let warmup: u64 = arguments.next().unwrap().parse().unwrap();
            let iterations: u64 = arguments.next().unwrap().parse().unwrap();
            assert!(arguments.next().is_none());
            let mut seed = signed as u64;
            let mut digest = 0u64;
            for _ in 0..warmup {{
                seed = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
                digest ^= std::hint::black_box(measure_entry(std::hint::black_box(seed)));
            }}
            let start = std::time::Instant::now();
            for _ in 0..iterations {{
                seed = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
                digest ^= std::hint::black_box(measure_entry(std::hint::black_box(seed)));
            }}
            let elapsed = start.elapsed().as_nanos();
            std::hint::black_box(digest);
            println!("sample {{elapsed}} {{iterations}} {{digest}}");
        }}
        _ => panic!("expected check or sample"),
    }}
}}
"###
    )
}

fn canonical(path: &Path) -> Result<PathBuf, HarnessError> {
    path.canonicalize().map_err(|source| HarnessError::Io {
        path: path.to_owned(),
        source,
    })
}

fn write(path: &Path, contents: &str) -> Result<(), HarnessError> {
    fs::write(path, contents).map_err(|source| HarnessError::Io {
        path: path.to_owned(),
        source,
    })
}

fn cargo_failure(output: &Output) -> String {
    let diagnostics = output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<CargoMessage>(line).ok())
        .filter_map(|message| message.message?.rendered)
        .collect::<Vec<_>>()
        .join("");
    if diagnostics.is_empty() {
        command_failure(output)
    } else {
        diagnostics
    }
}

fn command_failure(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("{stdout}{stderr}")
}

fn parse<T: std::str::FromStr>(value: Option<&str>, line: &str) -> Result<T, HarnessError> {
    value
        .ok_or_else(|| HarnessError::Output(line.to_owned()))?
        .parse()
        .map_err(|_| HarnessError::Output(line.to_owned()))
}

fn identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
