//! error: recursive enum roots cannot declare an unknown terminal body

use wire_repr::WireView;

#[derive(WireView)]
struct Body<T> {
    count: u8,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<T>,
}

#[derive(WireView)]
#[wire(selector = u8)]
enum Root {
    #[wire(value = 1)]
    Array(Body<Root>),
    #[wire(unknown)]
    Unknown(wire_repr::wire::Bytes),
}
