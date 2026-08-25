//! error: schema structs support one terminal nested schema field

use wire_repr::WireView;

#[derive(WireView)]
struct Bar {
    bar: u8,
}

#[derive(WireView)]
struct Foo {
    bar: Bar,
    foo: u8,
}
