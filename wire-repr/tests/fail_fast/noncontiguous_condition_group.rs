//! error: conditional group fields must be contiguous immediately after their logical flag

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct Foo {
    #[wire(as = u8)]
    present: bool,
    #[wire(flag = present)]
    details: bool,
    #[wire(depends_on = details)]
    first: u8,
    middle: u8,
    #[wire(depends_on = details)]
    second: u8,
}
