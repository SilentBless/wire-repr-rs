//! error: `rest` is only valid on the final physical field

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    #[wire(rest)]
    body: wire_repr::wire::Bytes,
    tail: u8,
}
