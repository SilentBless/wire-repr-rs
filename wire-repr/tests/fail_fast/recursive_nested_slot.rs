//! error: recursive enum body must pass the root directly to its recursive schema slot

use wire_repr::WireView;

struct Wrapper<T>(T);

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
    Array(Body<Wrapper<Root>>),
}
