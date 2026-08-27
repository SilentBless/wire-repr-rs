//! error: selection paths require named fields

use wire_repr::{ByteSelection, WireBuilder, WireView};

fn checksum(selection: impl ByteSelection) -> u16 {
    selection.bytes().map(u16::from).sum()
}

#[derive(WireView, WireBuilder)]
struct Child {
    value: u8,
}

#[derive(WireView, WireBuilder)]
struct Foo {
    #[wire(le, computed = checksum(include(child.0)))]
    checksum: u16,
    child: Child,
}
