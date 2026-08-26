//! error: computed fields cannot control representation geometry

use wire_repr::{ByteSelection, WireBuilder, WireView};

fn length(selection: impl ByteSelection) -> u8 {
    selection.len() as u8
}

#[derive(WireView, WireBuilder)]
struct Foo {
    #[wire(computed = length(include(body)))]
    length: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
}
