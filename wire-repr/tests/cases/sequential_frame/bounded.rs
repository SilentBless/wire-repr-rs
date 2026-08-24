#![deny(missing_docs, unsafe_code)]
//! Length-controlled borrowed byte field coverage.

#[cfg(not(feature = "bytes"))]
use wire_repr::PreparedLayout;
#[cfg(feature = "bytes")]
use wire_repr::ViewCursorError;
use wire_repr::Wire;

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
    assert!(matches!(
        Packet::view(&input),
        Err(PacketDecodeError::TrailingBytes {
            expected: 6,
            actual: 7,
        })
    ));

    let parsed = Packet::view(&input[..6]).unwrap();

    assert_eq!(parsed.as_bytes(), &input[..6]);
    assert_eq!(parsed.payload(), &input[2..5]);
    assert!(core::ptr::eq(
        parsed.payload().as_ptr(),
        input[2..5].as_ptr()
    ));
    assert_eq!(parsed.tail(), 9);

    let mut cursor = Packet::cursor(&input);
    assert_eq!(cursor.next().unwrap().unwrap().as_bytes(), &input[..6]);
    assert_eq!(cursor.remaining(), &input[6..]);
}

#[test]
fn bounded_bytes_report_truncation_and_platform_overflow() {
    let Err(error) = Packet::view(&[7, 3, 10, 11]) else {
        panic!("truncated payload was accepted");
    };
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
        let Err(error) = WidePacket::view(&input) else {
            panic!("unrepresentable length was accepted");
        };
        assert!(matches!(
            error,
            WidePacketDecodeError::LengthNotRepresentable { field: "payload" }
        ));
    }
}

#[test]
#[cfg(not(feature = "bytes"))]
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

    let parsed = Packet::view(written.as_bytes()).unwrap();
    assert_eq!(parsed.payload_length(), 3);
    assert_eq!(parsed.payload(), payload);
}

#[cfg(feature = "bytes")]
#[test]
fn bounded_bytes_retain_bytes_storage() {
    let backing = bytes::Bytes::from_static(&[7, 3, 10, 11, 12, 9]);
    let pointer = backing.as_ptr();
    let parsed = Packet::view(backing).unwrap();

    assert_eq!(parsed.as_bytes().as_ptr(), pointer);
    assert_eq!(parsed.payload().as_ptr(), pointer.wrapping_add(2));

    let short = [7, 3, 10, 11];
    let mut cursor = Packet::cursor(&short);
    assert!(matches!(
        cursor.next(),
        Err(ViewCursorError::Item(
            PacketDecodeError::InputTooShort { .. }
        ))
    ));
    assert_eq!(cursor.remaining(), &short);
}
