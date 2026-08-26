//! error: controller `length` cannot control both byte length and field position before the controller DAG ships

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct Foo {
    length: u8,
    #[wire(at = length)]
    marker: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
}
