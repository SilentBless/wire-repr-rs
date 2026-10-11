//! error: `sleb128` does not accept an endian attribute

use wire_repr::WireView;

#[derive(WireView)]
struct OrderedSigned {
    #[wire(sleb128, le)]
    value: i16,
}
