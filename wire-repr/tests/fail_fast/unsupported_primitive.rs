//! error: this Rust type requires an explicit unsigned `as` wire type

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    foo: bool,
}
