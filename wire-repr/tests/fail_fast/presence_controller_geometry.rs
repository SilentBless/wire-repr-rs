//! error: presence controller must have fixed sequential geometry

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct Foo {
    head: u8,
    #[wire(as = u8, pad_before = 1)]
    present: bool,
    #[wire(flag = present)]
    details: bool,
    #[wire(depends_on = details)]
    value: u8,
}
