//! error: `vu128` does not accept an endian attribute

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct OrderedPrefix {
    #[wire(vu128, zigzag, be)]
    value: i32,
}
