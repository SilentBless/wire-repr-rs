//! error: one-byte scalar fields do not accept an endian attribute

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    #[wire(le)]
    foo: u8,
}
