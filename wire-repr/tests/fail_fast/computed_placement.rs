//! error: computed destinations cannot declare placement geometry

use wire_repr::{ByteSelection, WireBuilder, WireView};

fn checksum(selection: impl ByteSelection) -> u16 {
    selection.bytes().map(u16::from).sum()
}

#[derive(WireView, WireBuilder)]
struct Foo {
    lead: u8,
    #[wire(at = 4, le, computed = checksum(exclude(self)))]
    checksum: u16,
}
