//! error: is not an iterator

use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Bar {
    value: u8,
}

#[derive(WireBuilder)]
struct Foo {
    #[wire(be)]
    count: i32,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<Bar>,
}

fn main() {
    let mut output = [0u8; 5];
    let _ = Foo::builder(&mut output[..]).count(1);
}
