//! error: bit range exceeds the physical representation

use wire_repr::WireView;

#[derive(WireView)]
#[wire(as = u8)]
struct Foo {
    #[wire(bit = 8)]
    value: bool,
}
