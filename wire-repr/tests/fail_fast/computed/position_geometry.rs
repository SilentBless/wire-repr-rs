//! error: computed position source cannot depend on physical geometry

use wire_repr::{ByteSourceCursor, Wire};

fn offset(_: &impl ByteSourceCursor) -> usize {
    0
}

#[derive(Wire)]
struct Packet {
    #[wire(computed = offset(exclude(marker)))]
    offset: u8,
    marker: u8,
    #[wire(at = offset)]
    payload: u8,
}
