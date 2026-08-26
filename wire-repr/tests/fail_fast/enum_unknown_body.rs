//! error: unknown variant body must be wire::Bytes

use wire_repr::WireView;

struct Bar;

#[derive(WireView)]
#[wire(selector = u8)]
enum Foo {
    #[wire(value = 1)]
    First(Bar),
    #[wire(unknown)]
    Unknown(Bar),
}
