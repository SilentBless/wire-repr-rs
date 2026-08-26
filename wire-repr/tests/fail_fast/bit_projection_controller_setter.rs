//! error: no method named `raw`

use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Foo {
    raw: u8,
    #[wire(bits_of = raw, bit = 0)]
    value: bool,
}

fn main() {
    let mut output = [0u8; 1];
    let _ = Foo::builder(&mut output[..]).raw(1);
}
