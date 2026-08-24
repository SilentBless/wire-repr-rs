//! error: wire validators must return `Result<(), Error>`

use wire_repr::validator;

struct ValidationError;

#[validator]
fn validate(_: u8) -> Result<u8, ValidationError> {
    Ok(1)
}
