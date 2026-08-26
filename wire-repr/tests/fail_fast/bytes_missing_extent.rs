//! error: `wire::Bytes` requires `bytes = earlier_field` or `rest`

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    body: wire_repr::wire::Bytes,
}
