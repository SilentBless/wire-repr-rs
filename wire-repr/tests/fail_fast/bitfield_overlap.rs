//! error: bitfield ranges cannot overlap

use wire_repr::WireView;

#[derive(WireView)]
#[wire(as = u8)]
struct Foo {
    #[wire(bits = 0..=2)]
    first: u8,
    #[wire(bits = 2..=3)]
    second: u8,
}
