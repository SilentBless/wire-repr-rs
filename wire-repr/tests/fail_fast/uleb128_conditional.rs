//! error: conditional fields cannot use `uleb128`

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct OptionalValue {
    #[wire(as = u8)]
    present: bool,
    #[wire(flag = present)]
    details: bool,
    #[wire(uleb128, depends_on = details)]
    value: u16,
}
