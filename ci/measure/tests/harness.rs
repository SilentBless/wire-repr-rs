use std::collections::BTreeMap;

use wire_repr_measure::measure::artifact::Analyzer;
use wire_repr_measure::measure::harness::{HarnessBuilder, HarnessEntry};

#[test]
fn builds_one_harness_for_multiple_sources_and_entries() {
    let root = std::env::temp_dir().join(format!(
        "wire-repr-measure-harness-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let generated = root.join("generated.rs");
    std::fs::write(
        &generated,
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
    let idiomatic = root.join("idiomatic.rs");
    std::fs::write(
        &idiomatic,
        "pub fn decode(seed: u64) -> u64 { seed.wrapping_add(5) }\n",
    )
    .unwrap();

    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let sources = BTreeMap::from([
        ("generated".to_owned(), generated),
        ("idiomatic".to_owned(), idiomatic),
    ]);
    let entries = [
        HarnessEntry::new("measure_entry_generated_decode", "generated", "decode"),
        HarnessEntry::new("measure_entry_idiomatic_decode", "idiomatic", "decode"),
    ];
    let harness = HarnessBuilder::new(workspace, root.join("target"), "1.91.0")
        .build(&sources, &entries)
        .unwrap();
    let generated = harness.entry("measure_entry_generated_decode").unwrap();
    let idiomatic = harness.entry("measure_entry_idiomatic_decode").unwrap();

    assert_eq!(generated.check(&[0, 1, -1]).unwrap()[1], (1, 128));
    assert_eq!(idiomatic.check(&[0, 1, -1]).unwrap()[1], (1, 6));
    let sample = generated.sample(1, 10, 100).unwrap();
    assert_eq!(sample.iterations, 100);
    assert!(sample.elapsed_ns > 0);
    assert_eq!(generated.executable(), idiomatic.executable());
    #[cfg(target_os = "windows")]
    assert!(generated.debug_info().is_some());
    let analyzer = Analyzer::open(generated.executable()).unwrap();
    let metrics = analyzer.analyze(generated.symbol()).unwrap();
    assert!(metrics.transitive_indirect_calls > 0);
    #[cfg(not(target_os = "windows"))]
    assert!(metrics.transitive_allocation_symbols > 0);
    assert_eq!(
        analyzer.analyze(idiomatic.symbol()).unwrap().indirect_calls,
        0
    );

    std::fs::remove_dir_all(root).unwrap();
}
