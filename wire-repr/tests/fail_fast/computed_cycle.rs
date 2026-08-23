//! error: dependency cycle

use wire_repr::{ByteSourceCursor, Wire};

fn count(source: &impl ByteSourceCursor) -> usize {
    source.byte_len()
}

#[derive(Wire)]
struct Packet {
    #[wire(computed = count(include(second)))]
    first: u8,
    #[wire(computed = count(include(first)))]
    second: u8,
}
