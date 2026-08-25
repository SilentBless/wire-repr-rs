//! error: cannot find value `bar`

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
#[wire(validate = bar)]
struct Foo {
    foo: u8,
}
