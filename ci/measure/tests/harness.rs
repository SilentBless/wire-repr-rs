use wire_repr_measure::measure::artifact::Analyzer;
use wire_repr_measure::measure::harness::HarnessBuilder;

#[test]
fn builds_and_executes_an_isolated_role_harness() {
    let root = std::env::temp_dir().join(format!(
        "wire-repr-measure-harness-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let role = root.join("generated.rs");
    std::fs::write(
        &role,
        r#"
fn rotate(seed: u64) -> u64 { seed.rotate_left(7) }
#[unsafe(no_mangle)]
#[inline(never)]
pub fn tail_helper(callback: fn(u64) -> u64, seed: u64) -> u64 {
    let seed = std::hint::black_box(Box::new(seed));
    callback(*seed)
}
pub fn decode(seed: u64) -> u64 { tail_helper(rotate, seed) }
"#,
    )
    .unwrap();

    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let harness = HarnessBuilder::new(workspace, root.join("target"), "1.91.0")
        .build(&role, "decode")
        .unwrap();

    assert_eq!(harness.check(&[0, 1, -1]).unwrap()[1], (1, 128));
    let sample = harness.sample(1, 10, 100).unwrap();
    assert_eq!(sample.iterations, 100);
    assert!(sample.elapsed_ns > 0);
    assert!(harness.executable().is_file());
    assert!(harness.assembly().is_file());
    let metrics = Analyzer::open(harness.executable())
        .unwrap()
        .analyze("measure_entry")
        .unwrap();
    assert!(metrics.tail_calls > 0);
    assert!(metrics.transitive_indirect_calls > 0);
    assert!(metrics.transitive_allocation_symbols > 0);

    std::fs::remove_dir_all(root).unwrap();
}
