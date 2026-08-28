//! Minimal end-to-end schema with a constant, a derived byte-length controller, an exact-source
//! view, and a growable progressive writer.

use wire_repr::{WireBuilder, WireView, wire};

#[allow(dead_code)]
#[derive(WireView, WireBuilder)]
struct Packet {
    #[wire(be, constant = 0x5752)]
    magic: u16,
    kind: u8,
    #[wire(be)]
    payload_len: u16,
    #[wire(bytes = payload_len)]
    payload: wire::Bytes,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = [0x57, 0x52, 7, 0, 5, b'h', b'e', b'l', b'l', b'o'];

    let packet = Packet::view(&input)?;
    assert_eq!(packet.magic(), 0x5752);
    assert_eq!(packet.kind(), 7);
    assert_eq!(packet.payload(), b"hello");
    assert_eq!(packet.as_bytes(), input);

    let mut output = Vec::new();
    let written = Packet::builder(&mut output)
        .kind(7)?
        .payload(&b"hello"[..])?
        .finish()?;

    assert_eq!(written.as_bytes(), input);
    Ok(())
}
