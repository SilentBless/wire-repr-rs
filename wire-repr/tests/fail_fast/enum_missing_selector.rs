//! error: static enum requires #[wire(selector = unsigned_type)]

use wire_repr::WireView;

struct Bar;

#[derive(WireView)]
enum Foo {
    #[wire(value = 1)]
    First(Bar),
}
