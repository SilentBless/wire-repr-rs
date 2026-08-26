//! error: item count controller cannot control another dependency role

use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Bar {
    value: u8,
}

#[derive(WireBuilder)]
struct Foo {
    count: u8,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<Bar>,
    #[wire(at = count)]
    tail: u8,
}
