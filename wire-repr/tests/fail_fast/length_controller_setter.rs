//! error: no method named `length`

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct Foo {
    length: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
}

fn main() {
    let mut output = [0u8; 2];
    let _ = Foo::builder(&mut output[..]).length(1);
}
