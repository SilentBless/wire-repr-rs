use std::collections::BTreeMap;

use wire_repr_measure::formula::FormulaEngine;
use wire_repr_measure::measure::artifact::Metrics as ArtifactMetrics;
use wire_repr_measure::measure::runtime::Summary as RuntimeSummary;
use wire_repr_measure::policy::{FormulaValue, evaluate};
use wire_repr_measure::report::{
    CaseReport, Report, RoleReport, RunSummary, WorkloadReport, render_human, render_json,
};
use wire_repr_measure::workload::Workload;

const SOURCE: &str = r#"
name = "fixed/scalars"
[roles]
generated = "generated.rs"
idiomatic = "idiomatic.rs"
best_latency = "best/latency.rs"
[[cases]]
name = "decode"
entry = "decode"
seeds = [1]
[[cases.formulas]]
name = "gap"
expression = "percent(generated_runtime_median_ns, best_latency_runtime_median_ns)"
unit = "%"
[[cases.rules]]
name = "idiomatic gate"
level = "error"
assert = "generated_runtime_median_ns <= idiomatic_runtime_median_ns"
[[cases.rules]]
name = "best attention"
level = "attention"
assert = "generated_runtime_median_ns <= best_latency_runtime_median_ns * 1.05"
message = "generated is materially behind best safe"
"#;

#[test]
fn evaluates_error_and_attention_rules_without_conflating_them() {
    let workload = Workload::parse("workload.toml".as_ref(), SOURCE).unwrap();
    let metrics = BTreeMap::from([
        ("generated_runtime_median_ns".to_owned(), 120.0),
        ("idiomatic_runtime_median_ns".to_owned(), 110.0),
        ("best_latency_runtime_median_ns".to_owned(), 100.0),
    ]);
    let result = evaluate(&workload.cases[0], &metrics, &FormulaEngine::new().unwrap()).unwrap();

    assert_eq!(result.formulas[0].value, 20.0);
    assert_eq!(result.findings.len(), 2);
    assert!(result.failed());
}

#[test]
fn human_is_concise_and_json_is_machine_stable() {
    let report = Report {
        compiler: "rustc test".to_owned(),
        target: "x86_64-test".to_owned(),
        workloads: vec![WorkloadReport {
            name: "fixed/scalars".to_owned(),
            cases: vec![CaseReport {
                name: "decode".to_owned(),
                equivalent: true,
                roles: BTreeMap::from([(
                    "generated".to_owned(),
                    RoleReport {
                        artifact: Some(ArtifactMetrics {
                            stack_bytes: Some(0),
                            ..ArtifactMetrics::default()
                        }),
                        runtime: Some(RuntimeSummary {
                            samples: 3,
                            median_ns: 1.0,
                            p95_ns: 1.1,
                            minimum_ns: 0.9,
                            maximum_ns: 1.1,
                            mad_ns: 0.1,
                        }),
                        custom: BTreeMap::from([("view_bytes".to_owned(), 16.0)]),
                    },
                )]),
                formulas: vec![FormulaValue {
                    name: "gap".to_owned(),
                    value: 0.0,
                    unit: "%".to_owned(),
                }],
                findings: Vec::new(),
            }],
        }],
        summary: RunSummary {
            errors: 0,
            attentions: 0,
            information: 0,
        },
    };

    let human = render_human(&report, false);
    assert!(human.contains("PASS fixed/scalars/decode"));
    assert!(!human.trim_start().starts_with('{'));
    let verbose = render_human(&report, true);
    assert!(verbose.contains("custom=[view_bytes=16.000]"));
    assert!(verbose.contains("gap=0.000%"));

    let json = render_json(&report).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["schema"], 1);
    assert_eq!(value["workloads"][0]["name"], "fixed/scalars");
}
