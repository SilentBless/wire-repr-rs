//! error: computed field dependency cycle

use wire_repr::{WireBuilder, WireView};

fn copy(value: u16) -> u16 {
    value
}

#[derive(WireView, WireBuilder)]
struct Foo {
    #[wire(le, computed = copy(second))]
    first: u16,
    #[wire(le, computed = copy(first))]
    second: u16,
}
