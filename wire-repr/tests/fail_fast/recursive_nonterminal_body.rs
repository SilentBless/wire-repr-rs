//! error: recursive view layout for `Body<Root>` is not available

use wire_repr::WireView;

#[derive(WireView)]
struct Body<T> {
    count: u8,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<T>,
    tail: u8,
}

#[derive(WireView)]
#[wire(selector = u8)]
enum Root {
    #[wire(value = 1)]
    Array(Body<Root>),
}
