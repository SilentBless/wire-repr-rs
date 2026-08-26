//! error: static `at` position 0 precedes cursor 1

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    head: u8,
    #[wire(at = 0)]
    tail: u8,
}
