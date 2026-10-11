//! error: item count controller `count` must be an integer scalar

use wire_repr::WireView;

#[derive(WireView)]
struct Item {
    value: u8,
}

#[derive(WireView)]
struct Counted {
    #[wire(le)]
    count: f32,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<Item>,
}
