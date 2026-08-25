use std::process::Command;

#[test]
fn list_command_uses_product_owned_workload_root() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wire-repr-measure"))
        .arg("list")
        .current_dir(workspace)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("fixed/scalars"));
    assert!(stdout.contains("generic/child"));
    assert!(stdout.contains("logical/conversions"));
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
