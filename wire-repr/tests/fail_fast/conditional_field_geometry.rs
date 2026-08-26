//! error: conditional dependent fields cannot declare independent geometry

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct Foo {
    #[wire(as = u8)]
    present: bool,
    #[wire(flag = present)]
    details: bool,
    #[wire(depends_on = details, pad_before = 1)]
    value: u8,
}
