//! error: fixed byte arrays do not accept endian or `as` attributes

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    #[wire(le)]
    foo: [u8; 4],
}
