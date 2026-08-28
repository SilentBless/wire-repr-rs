use std::collections::BTreeSet;
use std::process::Command;

#[test]
fn list_json_is_a_unique_product_owned_workload_matrix() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wire-repr-measure"))
        .args(["list", "--json"])
        .current_dir(workspace)
        .output()
        .unwrap();

    assert!(output.status.success());
    let names: Vec<String> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(names.len(), names.iter().collect::<BTreeSet<_>>().len());
    assert!(names.iter().any(|name| name == "fixed/scalars"));
    assert!(names.iter().any(|name| name == "generic/child"));
    assert!(names.iter().any(|name| name == "logical/conversions"));
}

#[test]
fn run_command_emits_one_json_document() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wire-repr-measure"))
        .args(["run", "--filter", "fixed/scalars/constant_build", "--json"])
        .current_dir(workspace)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema"], 1);
    assert_eq!(value["summary"]["errors"], 0);
}

#[test]
fn exact_workload_rejects_unknown_inventory_name() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wire-repr-measure"))
        .args([
            "run",
            "--workload",
            "missing/workload",
            "--no-runtime",
            "--json",
        ])
        .current_dir(workspace)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("selected no cases"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
