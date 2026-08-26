//! error: bit projection controller cannot control another dependency role

use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Foo {
    raw: u8,
    #[wire(bits_of = raw, bit = 0)]
    value: bool,
    #[wire(bytes = raw)]
    body: wire_repr::wire::Bytes,
}
