//! error: recursive demand body currently requires controller, child, bounded bytes, scalar, child

use wire_repr::WireBuilder;

#[derive(WireBuilder)]
struct Bad<T> {
    left: wire_repr::wire::Recursive<T>,
    #[wire(rest)]
    bytes: wire_repr::wire::Bytes,
}
