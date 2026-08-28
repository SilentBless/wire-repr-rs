//! error: recursive demand placement is supported only as padding/alignment before the fixed scalar

use wire_repr::WireView;

#[derive(WireView)]
struct Pair<T> {
    #[wire(le)]
    length: u64,
    left: wire_repr::wire::Recursive<T>,
    #[wire(bytes = length)]
    metadata: wire_repr::wire::Bytes,
    #[wire(le, at = 32)]
    opcode: u16,
    right: wire_repr::wire::Recursive<T>,
}
