//! error: item count controller `count` must be physically earlier

use wire_repr::WireView;

struct Bar;

#[derive(WireView)]
struct Foo {
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<Bar>,
    count: u8,
}
