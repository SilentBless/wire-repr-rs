//! error: computed destinations cannot be conditional

use wire_repr::{ByteSelection, WireBuilder, WireView};

fn checksum(selection: impl ByteSelection) -> u8 {
    selection.bytes().fold(0u8, u8::wrapping_add)
}

#[derive(WireView, WireBuilder)]
struct Foo {
    #[wire(as = u8)]
    present: bool,
    #[wire(flag = present)]
    details: bool,
    #[wire(depends_on = details, computed = checksum(exclude(self)))]
    checksum: u8,
    tail: u8,
}
