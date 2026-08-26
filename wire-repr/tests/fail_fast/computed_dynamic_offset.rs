//! error: computed destinations must have a fixed offset before demand geometry

use wire_repr::{ByteSelection, WireBuilder, WireView};

fn checksum(selection: impl ByteSelection) -> u16 {
    selection.bytes().map(u16::from).sum()
}

#[derive(WireView, WireBuilder)]
struct Foo {
    length: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
    #[wire(le, computed = checksum(exclude(self)))]
    checksum: u16,
}
