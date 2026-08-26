//! error: no method named `checksum`

use wire_repr::{ByteSelection, WireBuilder, WireView};

fn checksum(selection: impl ByteSelection) -> u16 {
    selection.bytes().map(u16::from).sum()
}

#[derive(WireView, WireBuilder)]
struct Foo {
    first: u8,
    #[wire(le, computed = checksum(exclude(self)))]
    checksum: u16,
}

fn main() {
    let mut output = [0u8; 3];
    let _ = Foo::builder(&mut output[..]).checksum(1);
}
