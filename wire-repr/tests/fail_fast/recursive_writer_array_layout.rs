//! error: recursive writer layout for `Array<Value>` is not available

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct Leaf {
    value: u8,
}

#[derive(WireBuilder)]
struct Array<T> {
    count: u8,
    #[wire(counted_by = count, at = 4)]
    items: wire_repr::wire::Array<T>,
}

#[derive(WireBuilder)]
#[wire(selector = u8)]
enum Value {
    #[wire(value = 1)]
    Leaf(Leaf),
    #[wire(value = 2)]
    Array(Array<Value>),
}
