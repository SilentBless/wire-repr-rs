//! Complete fixed IPv4 header with nominal bitfields and a computed Internet checksum.

#![allow(dead_code)]

use wire_repr::{ByteSelection, WireBuilder, WireView, select};

#[derive(WireView, WireBuilder)]
#[wire(as = u8)]
struct VersionIhl {
    #[wire(bits = 4..=7)]
    version: u8,
    #[wire(bits = 0..=3)]
    ihl: u8,
}

#[derive(WireView, WireBuilder)]
#[wire(as = u16, be)]
struct FlagsFragment {
    #[wire(bit = 15)]
    reserved: bool,
    #[wire(bit = 14)]
    dont_fragment: bool,
    #[wire(bit = 13)]
    more_fragments: bool,
    #[wire(bits = 0..=12)]
    fragment_offset: u16,
}

fn internet_checksum(selection: impl ByteSelection) -> u16 {
    let mut bytes = selection.bytes();
    let mut sum = 0u32;
    while let Some(high) = bytes.next() {
        let low = bytes.next().unwrap_or(0);
        sum += u32::from(u16::from_be_bytes([high, low]));
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

#[derive(WireView, WireBuilder)]
struct Ipv4Header {
    version_ihl: VersionIhl,
    dscp_ecn: u8,
    #[wire(be)]
    total_length: u16,
    #[wire(be)]
    identification: u16,
    flags_fragment: FlagsFragment,
    ttl: u8,
    protocol: u8,
    #[wire(be, computed = internet_checksum(exclude(self)))]
    header_checksum: u16,
    source: [u8; 4],
    destination: [u8; 4],
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = [
        0x45, 0x00, 0x00, 0x54, 0xa6, 0xf2, 0x40, 0x00, 0x40, 0x01, 0xc2, 0xfd, 0xc0, 0xa8, 0x00,
        0x01, 0x08, 0x08, 0x08, 0x08,
    ];
    let view = Ipv4Header::view(input)?;
    assert_eq!(view.version_ihl().version(), 4);
    assert!(view.flags_fragment().dont_fragment());
    assert_eq!(view.header_checksum(), 0xc2fd);
    assert_eq!(
        internet_checksum(select(&view).exclude(|fields| fields.header_checksum)),
        view.header_checksum()
    );

    let mut output = [0u8; 20];
    Ipv4Header::builder(&mut output[..])
        .version_ihl(|bits| bits.version(4).ihl(5))?
        .dscp_ecn(0)?
        .total_length(0x54)?
        .identification(0xa6f2)?
        .flags_fragment(|bits| {
            bits.reserved(false)
                .dont_fragment(true)
                .more_fragments(false)
                .fragment_offset(0)
        })?
        .ttl(64)?
        .protocol(1)?
        .source([192, 168, 0, 1])?
        .destination([8, 8, 8, 8])?
        .finish()?;
    assert_eq!(output, input);
    Ok(())
}
