//! error: `wire::Array<T>` requires `counted_by = earlier_field`

use wire_repr::WireView;

struct Bar;

#[derive(WireView)]
struct Foo {
    items: wire_repr::wire::Array<Bar>,
}
