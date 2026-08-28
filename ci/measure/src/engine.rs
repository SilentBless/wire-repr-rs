use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use thiserror::Error;

use crate::formula::FormulaEngine;
use crate::measure::artifact::{Analyzer, ArtifactError, Metrics as ArtifactMetrics};
use crate::measure::harness::{Harness, HarnessBuilder, HarnessError};
use crate::measure::runtime::{RuntimeError, calibration_next, interleaved_roles, summarize};
use crate::policy::{Finding, PolicyError, evaluate};
use crate::report::{CaseReport, Report, RoleReport, RunSummary, WorkloadReport};
use crate::workload::{Level, WorkloadError, discover};

#[derive(Clone, Debug)]
pub struct Options {
    pub workspace: PathBuf,
    pub workloads: PathBuf,
    pub target: PathBuf,
    pub toolchain: String,
    pub workload: Option<String>,
    pub filter: Option<String>,
    pub runtime: bool,
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    Workload(#[from] WorkloadError),
    #[error(transparent)]
    Harness(#[from] HarnessError),
    #[error(transparent)]
    Artifact(#[from] ArtifactError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Policy(#[from] PolicyError),
    #[error("failed to query compiler: {0}")]
    Compiler(String),
    #[error("workload filter selected no cases")]
    EmptySelection,
}

pub fn run(options: &Options) -> Result<Report, EngineError> {
    let discovered = discover(&options.workloads)?;
    let compiler = compiler(&options.toolchain)?;
    let target = compiler
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap_or("unknown")
        .to_owned();
    let builder = HarnessBuilder::new(
        &options.workspace,
        options.target.join("harness"),
        &options.toolchain,
    );
    let formulas =
        FormulaEngine::new().map_err(|source| EngineError::Compiler(source.to_string()))?;
    let mut workload_reports = Vec::new();

    for workload in discovered {
        if options
            .workload
            .as_ref()
            .is_some_and(|selected| workload.config.name != *selected)
        {
            continue;
        }
        let mut probe_metrics: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
        for probe in &workload.probes {
            let harness = builder.build(&probe.source, &probe.config.entry)?;
            let value = harness.check(&[0])?[0].1 as f64;
            probe_metrics
                .entry(probe.config.role.clone())
                .or_default()
                .insert(probe.config.name.clone(), value);
        }
        let mut cases = Vec::new();
        for case in &workload.config.cases {
            let identity = format!("{}/{}", workload.config.name, case.name);
            if options
                .filter
                .as_ref()
                .is_some_and(|filter| !identity.contains(filter))
            {
                continue;
            }
            let mut states = BTreeMap::new();
            for (role, source) in &workload.roles {
                let harness = builder.build(source, &case.entry)?;
                let artifact = Analyzer::open(harness.executable())?.analyze("measure_entry")?;
                states.insert(
                    role.clone(),
                    RoleState {
                        harness,
                        report: RoleReport {
                            artifact: Some(artifact),
                            runtime: None,
                            custom: probe_metrics.get(role).cloned().unwrap_or_default(),
                        },
                    },
                );
            }

            let mut equivalent = true;
            let mut expected = None;
            for state in states.values() {
                let values = state.harness.check(&case.seeds)?;
                if let Some(expected) = &expected {
                    equivalent &= expected == &values;
                } else {
                    expected = Some(values);
                }
            }

            if options.runtime {
                let roles = states.keys().cloned().collect::<Vec<_>>();
                let mut iterations = BTreeMap::new();
                for (role, state) in &states {
                    iterations.insert(
                        role.clone(),
                        calibrate(
                            &state.harness,
                            case.seeds[0],
                            case.runtime.warmup,
                            case.runtime.target_ms,
                        )?,
                    );
                }
                let mut samples = roles
                    .iter()
                    .map(|role| (role.clone(), Vec::with_capacity(case.runtime.samples)))
                    .collect::<BTreeMap<_, _>>();
                for sample in 0..case.runtime.samples {
                    let seed = case.seeds[sample % case.seeds.len()];
                    for role in interleaved_roles(&roles, sample) {
                        let state = &states[&role];
                        let iterations = iterations[&role];
                        let measured =
                            state
                                .harness
                                .sample(seed, case.runtime.warmup, iterations)?;
                        samples
                            .get_mut(&role)
                            .expect("sample role was initialized")
                            .push(measured.elapsed_ns as f64 / iterations as f64);
                    }
                }
                for (role, values) in samples {
                    states.get_mut(&role).expect("role exists").report.runtime =
                        Some(summarize(&values)?);
                }
            }

            let roles = states
                .into_iter()
                .map(|(role, state)| (role, state.report))
                .collect::<BTreeMap<_, _>>();
            let metrics = flatten(&roles);
            let mut evaluation = evaluate(case, &metrics, &formulas)?;
            if !equivalent {
                evaluation.findings.insert(
                    0,
                    Finding {
                        name: "semantic equivalence".to_owned(),
                        level: Level::Error,
                        message: "role outputs differ on declared seeds".to_owned(),
                    },
                );
            }
            cases.push(CaseReport {
                name: case.name.clone(),
                equivalent,
                roles,
                formulas: evaluation.formulas,
                findings: evaluation.findings,
            });
        }
        if !cases.is_empty() {
            workload_reports.push(WorkloadReport {
                name: workload.config.name,
                cases,
            });
        }
    }
    if workload_reports.is_empty() {
        return Err(EngineError::EmptySelection);
    }

    let mut summary = RunSummary::default();
    for finding in workload_reports
        .iter()
        .flat_map(|workload| &workload.cases)
        .flat_map(|case| &case.findings)
    {
        match finding.level {
            Level::Error => summary.errors += 1,
            Level::Attention => summary.attentions += 1,
            Level::Info => summary.information += 1,
        }
    }
    Ok(Report {
        compiler: compiler
            .lines()
            .next()
            .unwrap_or("rustc unknown")
            .to_owned(),
        target,
        workloads: workload_reports,
        summary,
    })
}

struct RoleState {
    harness: Harness,
    report: RoleReport,
}

fn flatten(roles: &BTreeMap<String, RoleReport>) -> BTreeMap<String, f64> {
    let mut values = BTreeMap::new();
    for (role, result) in roles {
        if let Some(artifact) = &result.artifact {
            insert_artifact(&mut values, role, artifact);
        }
        if let Some(runtime) = &result.runtime {
            for (name, value) in [
                ("samples", runtime.samples as f64),
                ("median_ns", runtime.median_ns),
                ("p95_ns", runtime.p95_ns),
                ("minimum_ns", runtime.minimum_ns),
                ("maximum_ns", runtime.maximum_ns),
                ("mad_ns", runtime.mad_ns),
            ] {
                values.insert(format!("{role}_runtime_{name}"), value);
            }
        }
        for (name, value) in &result.custom {
            values.insert(format!("{role}_custom_{name}"), *value);
        }
    }
    values
}

fn insert_artifact(values: &mut BTreeMap<String, f64>, role: &str, metrics: &ArtifactMetrics) {
    for (name, value) in [
        ("text_bytes", metrics.text_bytes),
        ("instructions", metrics.instructions),
        ("branches", metrics.branches),
        ("direct_calls", metrics.direct_calls),
        ("linkage_calls", metrics.linkage_calls),
        ("tail_calls", metrics.tail_calls),
        ("indirect_calls", metrics.indirect_calls),
        ("panic_paths", metrics.panic_paths),
        ("allocation_symbols", metrics.allocation_symbols),
        ("reachable_functions", metrics.reachable_functions),
        ("transitive_instructions", metrics.transitive_instructions),
        ("transitive_branches", metrics.transitive_branches),
        ("transitive_direct_calls", metrics.transitive_direct_calls),
        ("transitive_linkage_calls", metrics.transitive_linkage_calls),
        ("transitive_tail_calls", metrics.transitive_tail_calls),
        (
            "transitive_indirect_calls",
            metrics.transitive_indirect_calls,
        ),
        ("transitive_panic_paths", metrics.transitive_panic_paths),
        (
            "transitive_allocation_symbols",
            metrics.transitive_allocation_symbols,
        ),
        ("max_call_depth", metrics.max_call_depth),
    ] {
        values.insert(format!("{role}_artifact_{name}"), value as f64);
    }
    if let Some(stack) = metrics.stack_bytes {
        values.insert(format!("{role}_artifact_stack_bytes"), stack as f64);
    }
}
fn calibrate(
    harness: &Harness,
    seed: i64,
    warmup: u64,
    target_ms: u64,
) -> Result<u64, HarnessError> {
    let mut iterations = 100u64;
    for _ in 0..6 {
        let sample = harness.sample(seed, warmup, iterations)?;
        let next = calibration_next(iterations, sample.elapsed_ns, target_ms);
        if next.abs_diff(iterations) <= iterations / 10 {
            return Ok(next);
        }
        iterations = next.min(1_000_000_000);
    }
    Ok(iterations)
}

fn compiler(toolchain: &str) -> Result<String, EngineError> {
    let output = Command::new("rustc")
        .arg(format!("+{toolchain}"))
        .arg("-vV")
        .output()
        .map_err(|error| EngineError::Compiler(error.to_string()))?;
    if !output.status.success() {
        return Err(EngineError::Compiler(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| EngineError::Compiler(error.to_string()))
}
