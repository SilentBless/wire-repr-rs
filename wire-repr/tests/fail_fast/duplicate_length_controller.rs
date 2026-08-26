//! error: byte length controller `length` cannot control multiple fields before the controller DAG ships

use wire_repr::WireView;

#[derive(WireView)]
struct Foo {
    length: u8,
    #[wire(bytes = length)]
    first: wire_repr::wire::Bytes,
    #[wire(bytes = length)]
    second: wire_repr::wire::Bytes,
}
