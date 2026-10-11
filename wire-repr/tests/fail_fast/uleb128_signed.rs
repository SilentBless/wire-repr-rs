//! error: `uleb128` requires an unsigned physical scalar

use wire_repr::WireView;

#[derive(WireView)]
struct SignedValue {
    #[wire(uleb128)]
    value: i32,
}
