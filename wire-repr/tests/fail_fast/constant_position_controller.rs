//! error: field position controller `offset` cannot be a constant

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct Foo {
    #[wire(constant = 4)]
    offset: u8,
    #[wire(at = offset)]
    value: u8,
}
