use std::collections::BTreeMap;
use std::path::Path;

use wire_repr_measure::formula::FormulaEngine;
use wire_repr_measure::workload::{Level, Workload, discover};

const SOURCE: &str = r#"
name = "fixed/scalars"

[roles]
generated = "generated.rs"
idiomatic = "idiomatic.rs"
best_latency = "best/latency.rs"
floor = "floor.rs"

[[probes]]
name = "view_bytes"
role = "generated"
source = "metrics/view.rs"
entry = "view_bytes"

[[cases]]
name = "decode"
entry = "decode"
seeds = [0, 1, -1]

[cases.runtime]
samples = 11
target_ms = 5
warmup = 100

[[cases.formulas]]
name = "optimization_gap"
expression = "percent(generated_runtime_median_ns, best_latency_runtime_median_ns)"
unit = "%"

[[cases.rules]]
name = "no indirect calls"
level = "error"
assert = "generated_artifact_indirect_calls == 0"

[[cases.rules]]
name = "best attention"
level = "attention"
assert = "generated_runtime_median_ns <= best_latency_runtime_median_ns * 1.05"
"#;

#[test]
fn parses_directory_scoped_workload_contract() {
    let workload = Workload::parse(Path::new("fixed/scalars/workload.toml"), SOURCE).unwrap();

    assert_eq!(workload.name, "fixed/scalars");
    assert_eq!(workload.roles.len(), 4);
    assert_eq!(workload.cases.len(), 1);
    assert_eq!(workload.cases[0].runtime.samples, 11);
    assert_eq!(workload.cases[0].rules[0].level, Level::Error);
    assert_eq!(workload.cases[0].rules[1].level, Level::Attention);
}

#[test]
fn rejects_zone_names_that_cannot_round_trip_through_ci() {
    for name in ["-foo", "foo bar", "foo/$(command)", "foo\nbar", "foo/\"bar"] {
        let source = SOURCE.replacen("fixed/scalars", name, 1);
        assert!(
            Workload::parse(Path::new("workload.toml"), &source).is_err(),
            "{name:?}"
        );
    }
}

#[test]
fn evaluates_workload_formulas_with_named_metrics() {
    let engine = FormulaEngine::new().unwrap();
    let metrics = BTreeMap::from([
        ("generated_runtime_median_ns".to_owned(), 120.0),
        ("best_latency_runtime_median_ns".to_owned(), 100.0),
        ("generated_artifact_indirect_calls".to_owned(), 0.0),
    ]);

    assert_eq!(
        engine
            .number(
                "percent(generated_runtime_median_ns, best_latency_runtime_median_ns)",
                &metrics,
            )
            .unwrap(),
        20.0
    );
    assert!(
        engine
            .boolean("generated_artifact_indirect_calls == 0", &metrics)
            .unwrap()
    );
    assert_eq!(engine.number("ratio(120, 100)", &metrics).unwrap(), 1.2);
    assert_eq!(engine.number("delta(120, 100)", &metrics).unwrap(), 20.0);
    assert_eq!(engine.number("percent(0, 0)", &metrics).unwrap(), 0.0);
}

#[test]
fn rejects_formula_references_to_missing_metrics() {
    let engine = FormulaEngine::new().unwrap();
    let error = engine
        .number(
            "generated_runtime_median_ns / missing_metric",
            &BTreeMap::new(),
        )
        .unwrap_err();

    assert!(error.to_string().contains("missing_metric"));
}

#[test]
fn discovers_zoned_workloads_and_resolves_role_sources() {
    let root = std::env::temp_dir().join(format!(
        "wire-repr-measure-contract-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let zone = root.join("fixed/scalars");
    std::fs::create_dir_all(zone.join("best")).unwrap();
    std::fs::write(zone.join("workload.toml"), SOURCE).unwrap();
    for role in [
        "generated.rs",
        "idiomatic.rs",
        "best/latency.rs",
        "floor.rs",
    ] {
        std::fs::write(zone.join(role), "").unwrap();
    }
    std::fs::create_dir_all(zone.join("metrics")).unwrap();
    std::fs::write(zone.join("metrics/view.rs"), "").unwrap();

    let workloads = discover(&root).unwrap();
    assert_eq!(workloads.len(), 1);
    assert_eq!(workloads[0].config.name, "fixed/scalars");
    assert_eq!(
        workloads[0].roles["best_latency"],
        zone.join("best/latency.rs").canonicalize().unwrap()
    );
    assert_eq!(workloads[0].probes[0].config.name, "view_bytes");

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_invalid_roles_cases_and_runtime_policies() {
    let invalid = [
        r#"
name = "foo"
[roles]
generated = "generated.rs"
best_latency = "best.rs"
[[cases]]
name = "decode"
entry = "decode"
seeds = [1]
"#,
        r#"
name = "foo"
[roles]
generated = "generated.rs"
idiomatic = "idiomatic.rs"
[[cases]]
name = "decode"
entry = "decode"
seeds = [1]
"#,
        r#"
name = "foo"
[roles]
generated = "../generated.rs"
idiomatic = "idiomatic.rs"
best_latency = "best.rs"
[[cases]]
name = "decode"
entry = "decode"
seeds = [1]
"#,
        r#"
name = "foo"
[roles]
generated = "generated.rs"
idiomatic = "idiomatic.rs"
best_latency = "best.rs"
[[cases]]
name = "decode"
entry = "decode"
seeds = []
"#,
        r#"
name = "foo"
[roles]
generated = "generated.rs"
idiomatic = "idiomatic.rs"
best_latency = "best.rs"
[[cases]]
name = "decode"
entry = "decode"
seeds = [1]
[cases.runtime]
samples = 2
"#,
        r#"
name = "foo"
[roles]
generated = "metrics/role.rs"
idiomatic = "idiomatic.rs"
best_latency = "best.rs"
[[cases]]
name = "decode"
entry = "decode"
seeds = [1]
"#,
        r#"
name = "foo"
[roles]
generated = "generated.rs"
idiomatic = "idiomatic.rs"
best_latency = "best.rs"
[[probes]]
name = "state"
role = "generated"
source = "generated.rs"
entry = "state"
[[cases]]
name = "decode"
entry = "decode"
seeds = [1]
"#,
    ];

    for source in invalid {
        assert!(Workload::parse(Path::new("foo/workload.toml"), source).is_err());
    }
}

#[test]
fn discovery_rejects_empty_and_misnamed_zones() {
    let root = std::env::temp_dir().join(format!(
        "wire-repr-measure-invalid-discovery-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        discover(&root)
            .unwrap_err()
            .to_string()
            .contains("no workload")
    );

    let zone = root.join("fixed/foo");
    std::fs::create_dir_all(&zone).unwrap();
    std::fs::write(
        zone.join("workload.toml"),
        r#"
name = "wrong/name"
[roles]
generated = "generated.rs"
idiomatic = "idiomatic.rs"
best_latency = "best.rs"
[[cases]]
name = "decode"
entry = "decode"
seeds = [1]
"#,
    )
    .unwrap();
    assert!(
        discover(&root)
            .unwrap_err()
            .to_string()
            .contains("zone path")
    );

    std::fs::remove_dir_all(root).unwrap();
}
