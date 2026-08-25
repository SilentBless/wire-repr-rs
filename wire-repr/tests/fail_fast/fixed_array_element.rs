//! error: fixed wire arrays currently require `u8` elements

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    foo: [u16; 2],
}
