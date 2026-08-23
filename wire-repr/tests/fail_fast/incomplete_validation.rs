//! error: validators require exactly one `error = ErrorType`

use wire_repr::Wire;

fn validate(_: u8) -> Result<(), ()> {
    Ok(())
}

#[derive(Wire)]
struct Packet {
    #[wire(validate = validate)]
    value: u8,
}
