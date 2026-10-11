//! error: `uleb128` does not accept an endian attribute

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct OrderedValue {
    #[wire(uleb128, le)]
    value: u32,
}
