//! error: `sleb128` requires a signed physical integer

use wire_repr::WireView;

#[derive(WireView)]
struct UnsignedSignedEncoding {
    #[wire(sleb128)]
    value: u16,
}
