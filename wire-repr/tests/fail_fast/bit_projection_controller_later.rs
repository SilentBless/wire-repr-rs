//! error: bit projection controller must be physically earlier

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    #[wire(bits_of = raw, bit = 0)]
    value: bool,
    raw: u8,
}
