#![deny(missing_docs, unsafe_code)]
//! Length-controlled borrowed byte field coverage.

use wire_repr::{PreparedLayout, Wire};

/// A packet with a canonical one-byte payload length.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct Packet<'wire> {
    /// Packet kind.
    pub kind: u8,
    /// Encoded payload length.
    pub payload_length: u8,
    /// Borrowed payload bytes.
    #[wire(bytes = payload_length)]
    pub payload: &'wire [u8],
    /// Byte after the bounded payload.
    pub tail: u8,
}

/// A packet whose decoded length can exceed `usize`.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct WidePacket<'wire> {
    /// Encoded payload length.
    #[wire(be)]
    pub payload_length: u128,
    /// Borrowed payload bytes.
    #[wire(bytes = payload_length)]
    pub payload: &'wire [u8],
}

#[test]
fn bounded_bytes_preserve_framing_and_source_identity() {
    let input = [7, 3, 10, 11, 12, 9, 0xaa];
    let (parsed, suffix) = Packet::view(&input).with_remainder().unwrap();

    assert_eq!(parsed.as_bytes(), &input[..6]);
    assert_eq!(suffix, &input[6..]);
    assert_eq!(parsed.payload(), &input[2..5]);
    assert!(core::ptr::eq(
        parsed.payload().as_ptr(),
        input[2..5].as_ptr()
    ));
    assert_eq!(parsed.tail(), 9);
}

#[test]
fn bounded_bytes_report_truncation_and_platform_overflow() {
    let error = Packet::view(&[7, 3, 10, 11]).with_remainder().unwrap_err();
    assert!(matches!(
        error,
        PacketDecodeError::InputTooShort {
            field: "payload",
            required: 3,
            available: 2,
        }
    ));
    assert_eq!(
        error.to_string(),
        "field `payload` needs 3 bytes, but only 2 bytes remain"
    );

    if usize::BITS < 128 {
        let input = u128::MAX.to_be_bytes();
        let error = WidePacket::view(&input).with_remainder().unwrap_err();
        assert!(matches!(
            error,
            WidePacketDecodeError::LengthNotRepresentable {
                field: "payload",
                value: u128::MAX,
            }
        ));
    }
}

#[test]
fn preparation_derives_the_canonical_length_before_writing() {
    let payload = [1, 2, 3];
    let packet = Packet {
        kind: 4,
        payload_length: 99,
        payload: &payload,
        tail: 5,
    };
    let plan = packet.prepare().unwrap();
    assert_eq!(plan.encoded_len(), 6);

    let mut output = [0_u8; 8];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[4, 3, 1, 2, 3, 5]);
    assert_eq!(suffix, &mut [0, 0]);

    let parsed = Packet::view(written.as_bytes()).without_trailing().unwrap();
    assert_eq!(parsed.payload_length(), 3);
    assert_eq!(parsed.payload(), payload);
}

#[test]
fn preparation_and_short_commit_fail_without_mutation() {
    let oversized = [0_u8; 256];
    let error = match (Packet {
        kind: 1,
        payload_length: 0,
        payload: &oversized,
        tail: 2,
    }
    .prepare())
    {
        Err(error) => error,
        Ok(_) => panic!("oversized payload should fail during preparation"),
    };
    assert!(matches!(
        error,
        PacketEncodeError::LengthNotRepresentable {
            field: "payload",
            source: "payload_length",
            length: 256,
        }
    ));

    let payload = [6, 7, 8];
    let plan = Packet {
        kind: 1,
        payload_length: 0,
        payload: &payload,
        tail: 2,
    }
    .prepare()
    .unwrap();
    let mut short = [0xa5; 5];
    assert!(plan.commit_into(&mut short).is_err());
    assert_eq!(short, [0xa5; 5]);
}
