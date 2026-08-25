use std::collections::BTreeSet;

use super::{ArtifactError, MAX_REACHABLE_FUNCTIONS, register_function};

#[test]
fn traversal_limit_is_an_error_not_partial_evidence() {
    let mut visited = (0..MAX_REACHABLE_FUNCTIONS as u64).collect::<BTreeSet<_>>();
    let error = register_function(&mut visited, u64::MAX, "entry").unwrap_err();

    assert!(matches!(error, ArtifactError::TraversalLimit { .. }));
}
