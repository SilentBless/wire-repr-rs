//! error: wire enum requires at least one known variant

use wire_repr::WireView;

#[derive(WireView)]
#[wire(selector = u8)]
enum Foo {
    #[wire(unknown)]
    Fallback(wire_repr::wire::Bytes),
}
