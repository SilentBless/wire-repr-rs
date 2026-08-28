#![allow(unsafe_code)]

use wire_repr_measure::measure::artifact::Analyzer;

fn artifact_path() -> std::path::PathBuf {
    std::env::var_os("WIRE_REPR_MEASURE_TEST_ARTIFACT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_exe().unwrap())
}

fn triple(value: u64) -> u64 {
    value.wrapping_mul(3)
}

#[inline(never)]
fn wire_measure_helper(callback: fn(u64) -> u64, value: u64) -> u64 {
    let value = std::hint::black_box(Box::new(value));
    callback(*value)
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn wire_measure_probe(value: u64) -> u64 {
    if std::hint::black_box(value) == 0 {
        1
    } else {
        wire_measure_helper(triple, value)
    }
}

trait Dispatch {
    fn dispatch(&self, value: u64) -> u64;
}

struct DispatchImpl;

impl Dispatch for DispatchImpl {
    fn dispatch(&self, value: u64) -> u64 {
        value.wrapping_add(1)
    }
}

static DISPATCH: DispatchImpl = DispatchImpl;

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn wire_measure_vtable_probe(value: u64) -> u64 {
    let dispatch = std::hint::black_box(&DISPATCH as &dyn Dispatch);
    dispatch.dispatch(value)
}

#[test]
fn measures_a_symbol_from_the_linked_host_artifact() {
    assert_eq!(wire_measure_probe(2), 6);
    let analyzer = Analyzer::open(&artifact_path()).unwrap();
    let metrics = analyzer.analyze("wire_measure_probe").unwrap();

    assert!(metrics.text_bytes > 0);
    assert!(metrics.instructions > 0);
    assert!(metrics.branches > 0);
    assert_eq!(metrics.indirect_calls, 0);
    assert!(metrics.reachable_functions >= 2);
    assert!(metrics.transitive_indirect_calls > 0);
    assert!(metrics.transitive_allocation_symbols > 0);
}

#[test]
fn detects_trait_object_vtable_evidence_in_the_host_artifact() {
    assert_eq!(wire_measure_vtable_probe(2), 3);
    let analyzer = Analyzer::open(&artifact_path()).unwrap();
    let metrics = analyzer.analyze("wire_measure_vtable_probe").unwrap();

    assert!(metrics.indirect_calls > 0);
    assert!(metrics.vtable_references > 0);
}
