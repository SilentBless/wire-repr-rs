//! error: `uleb128` cannot be a field position controller

use wire_repr::WireView;

#[derive(WireView)]
struct VariableDestination {
    #[wire(uleb128)]
    destination: u16,
    #[wire(at = destination)]
    value: u8,
}
