//! error: byte length controller `length` must be physically earlier

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
    length: u8,
}
