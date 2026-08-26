//! error: `align_before` must be nonzero

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    head: u8,
    #[wire(align_before = 0)]
    tail: u8,
}
