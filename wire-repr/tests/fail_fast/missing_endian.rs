//! error: multi-byte scalar wire fields require #[wire(le)] or #[wire(be)]

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    foo: u16,
}
