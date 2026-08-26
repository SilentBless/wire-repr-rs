//! error: known enum variant requires #[wire(value = ...)]

use wire_repr::WireView;

struct Bar;

#[derive(WireView)]
#[wire(selector = u8)]
enum Foo {
    First(Bar),
}
