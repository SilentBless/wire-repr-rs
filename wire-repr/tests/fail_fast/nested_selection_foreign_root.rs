//! error: type mismatch resolving

use wire_repr::{WireView, select};

#[derive(WireView)]
struct Foo {
    value: u8,
}

#[derive(WireView)]
struct Bar {
    value: u8,
}

fn main() {
    let bar = Bar::view([2]).unwrap();
    let mut captured = None;
    let selected = select(&bar).include(|fields| {
        captured = Some(fields.value);
        fields.value
    });
    let _ = selected.len();
    let foreign = captured.unwrap();

    let foo = Foo::view([1]).unwrap();
    let _ = select(&foo).include(|_| foreign);
}
