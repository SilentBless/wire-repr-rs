use std::path::Path;

use wire_repr_measure::engine::{Options, run};

#[test]
fn runs_discovery_build_artifact_runtime_formula_and_policy_pipeline() {
    let root = std::env::temp_dir().join(format!(
        "wire-repr-measure-engine-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let zone = root.join("workloads/fixed/foo");
    std::fs::create_dir_all(zone.join("best")).unwrap();
    std::fs::create_dir_all(zone.join("metrics")).unwrap();
    std::fs::write(
        zone.join("workload.toml"),
        r#"
name = "fixed/foo"
[roles]
generated = "generated.rs"
idiomatic = "idiomatic.rs"
best_latency = "best/latency.rs"
[[probes]]
name = "state_bytes"
role = "generated"
source = "metrics/state.rs"
entry = "state_bytes"
[[cases]]
name = "decode"
entry = "decode"
seeds = [0, 1, -1]
[cases.runtime]
samples = 3
target_ms = 1
warmup = 10
[[cases.formulas]]
name = "gap"
expression = "percent(generated_runtime_median_ns, best_latency_runtime_median_ns)"
unit = "%"
[[cases.rules]]
name = "no dispatch"
level = "error"
assert = "generated_artifact_indirect_calls == 0"
"#,
    )
    .unwrap();
    let role = r#"
pub fn decode(seed: u64) -> u64 { seed.rotate_left(7) }
"#;
    std::fs::write(zone.join("generated.rs"), role).unwrap();
    std::fs::write(zone.join("idiomatic.rs"), role).unwrap();
    std::fs::write(zone.join("best/latency.rs"), role).unwrap();
    std::fs::write(
        zone.join("metrics/state.rs"),
        "pub fn state_bytes(_seed: u64) -> u64 { 8 }\n",
    )
    .unwrap();

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let report = run(&Options {
        workspace: workspace.to_owned(),
        workloads: root.join("workloads"),
        target: root.join("target"),
        toolchain: "1.91.0".to_owned(),
        filter: None,
        runtime: true,
    })
    .unwrap();

    assert_eq!(report.summary.errors, 0);
    assert_eq!(report.workloads[0].name, "fixed/foo");
    let case = &report.workloads[0].cases[0];
    assert!(case.equivalent);
    assert!(case.roles["generated"].artifact.is_some());
    assert!(case.roles["generated"].runtime.is_some());
    assert_eq!(case.roles["generated"].custom["state_bytes"], 8.0);
    assert_eq!(case.formulas[0].name, "gap");

    std::fs::remove_dir_all(root).unwrap();
}
