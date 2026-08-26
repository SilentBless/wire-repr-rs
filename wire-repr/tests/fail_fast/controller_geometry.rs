//! error: byte length controller `length` must have fixed sequential geometry

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    head: u8,
    #[wire(pad_before = 1)]
    length: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
}
