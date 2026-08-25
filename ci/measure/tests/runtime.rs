use wire_repr_measure::measure::runtime::{calibration_next, interleaved_roles, summarize};

#[test]
fn summarizes_samples_without_hiding_variance() {
    let summary = summarize(&[10.0, 11.0, 12.0, 13.0, 100.0]).unwrap();

    assert_eq!(summary.median_ns, 12.0);
    assert_eq!(summary.p95_ns, 100.0);
    assert_eq!(summary.minimum_ns, 10.0);
    assert_eq!(summary.maximum_ns, 100.0);
    assert_eq!(summary.mad_ns, 1.0);
}

#[test]
fn rotates_role_order_between_samples() {
    let roles = vec![
        "generated".to_owned(),
        "idiomatic".to_owned(),
        "best".to_owned(),
    ];
    assert_eq!(
        interleaved_roles(&roles, 0),
        ["generated", "idiomatic", "best"]
    );
    assert_eq!(
        interleaved_roles(&roles, 1),
        ["idiomatic", "best", "generated"]
    );
}

#[test]
fn calibration_grows_past_zero_duration_probes() {
    assert_eq!(calibration_next(1, 0, 2), 100);
    assert_eq!(calibration_next(100, 100_000, 2), 2_000);
    assert_eq!(calibration_next(10_000, 2_000_000, 2), 10_000);
}
