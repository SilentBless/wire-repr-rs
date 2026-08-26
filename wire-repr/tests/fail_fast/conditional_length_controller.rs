//! error: byte length controller `length` must have fixed sequential geometry

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct Foo {
    #[wire(as = u8)]
    present: bool,
    #[wire(flag = present)]
    details: bool,
    #[wire(depends_on = details)]
    length: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
}
