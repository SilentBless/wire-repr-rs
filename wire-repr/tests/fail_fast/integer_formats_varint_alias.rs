//! error: unsupported schema field attribute

use wire_repr::WireView;

#[derive(WireView)]
struct AmbiguousEncoding {
    #[wire(varint)]
    value: u32,
}
