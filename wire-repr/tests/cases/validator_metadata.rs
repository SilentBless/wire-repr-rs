//! Validator callback metadata coverage.

use wire_repr::validator;

/// A semantic validation failure.
#[derive(Debug, Eq, PartialEq)]
pub struct DomainError;

/// Rejects zero values.
#[validator]
pub fn nonzero(value: u8) -> Result<(), DomainError> {
    if value == 0 { Err(DomainError) } else { Ok(()) }
}

mod nested {
    use super::DomainError;
    use wire_repr::validator;

    /// A validator reachable through a qualified path.
    #[validator]
    pub fn qualified(value: u8) -> Result<(), DomainError> {
        super::nonzero(value)
    }
}

#[test]
fn validator_metadata_preserves_error_types_and_paths() {
    fn root_error(error: nonzero::Error) -> DomainError {
        error
    }
    fn nested_error(error: nested::qualified::Error) -> DomainError {
        error
    }

    assert_eq!(root_error(nonzero(0).unwrap_err()), DomainError);
    assert_eq!(nested_error(nested::qualified(0).unwrap_err()), DomainError);
}
