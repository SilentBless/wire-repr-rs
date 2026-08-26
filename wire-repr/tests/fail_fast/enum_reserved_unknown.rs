//! error: `Unknown` is reserved when an unknown fallback variant is present

use wire_repr::WireView;

struct Bar;

#[derive(WireView)]
#[wire(selector = u8)]
enum Foo {
    #[wire(value = 1)]
    Unknown(Bar),
    #[wire(unknown)]
    Fallback(wire_repr::wire::Bytes),
}
