//! error: no method named `present`

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct Foo {
    #[wire(as = u8)]
    present: bool,
    #[wire(flag = present)]
    details: bool,
    #[wire(depends_on = details)]
    value: u8,
}

fn main() {
    let mut output = [0u8; 2];
    let _ = Foo::builder(&mut output[..]).present(true);
}
