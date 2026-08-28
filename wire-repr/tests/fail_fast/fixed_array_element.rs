//! error: fixed wire arrays require primitive scalar elements

use wire_repr::WireView;
struct Item;

#[derive(WireView)]
struct Foo {
    foo: [Item; 2],
}
