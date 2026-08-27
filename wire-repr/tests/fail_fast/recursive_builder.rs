//! error: recursive enum builders are not supported by the current read-side recursive capability

use wire_repr::WireBuilder;

struct Body<T>(T);

#[derive(WireBuilder)]
#[wire(selector = u8)]
enum Root {
    #[wire(value = 1)]
    Array(Body<Root>),
}
