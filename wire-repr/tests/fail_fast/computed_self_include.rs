//! error: cannot include the computed field itself

use wire_repr::{ByteSourceCursor, Wire};

fn count(source: &impl ByteSourceCursor) -> usize {
    source.byte_len()
}

#[derive(Wire)]
struct Packet {
    #[wire(computed = count(include(checksum)))]
    checksum: u8,
}
