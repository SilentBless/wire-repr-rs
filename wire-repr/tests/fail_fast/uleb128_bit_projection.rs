//! error: `uleb128` cannot be a logical bit projection

use wire_repr::WireView;

#[derive(WireView)]
struct ProjectedValue {
    register: u8,
    #[wire(uleb128, bits_of = register, bits = 0..=3)]
    value: u8,
}
