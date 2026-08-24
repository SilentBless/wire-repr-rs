//! error: required by a bound in `requires_error`

use core::fmt;
use wire_repr::{Wire, validator};

#[derive(Debug)]
struct DomainError;

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("rejected")
    }
}

#[validator]
fn validate(_: u8) -> Result<(), DomainError> {
    Err(DomainError)
}

#[derive(Wire)]
struct Packet {
    #[wire(validate = validate)]
    value: u8,
}
