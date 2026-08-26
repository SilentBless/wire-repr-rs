//! error: bit projection controller `raw` must have fixed sequential geometry

use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Foo {
    length: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
    raw: u8,
    #[wire(bits_of = raw, bit = 0)]
    value: bool,
}
