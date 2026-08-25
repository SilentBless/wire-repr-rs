//! error: `foo` does not live long enough

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    foo: u8,
}

fn bar() -> impl FooView {
    let foo = [1];
    Foo::view(&foo).unwrap()
}
