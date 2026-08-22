#![deny(missing_docs, unsafe_code)]

//! Public nominal bitfield view and prepared-write coverage.

use wire_repr::{ByteSourceCursor, Computed, PreparedLayout, Wire};

/// A nominal big-endian flags representation.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(bitfield = u16, be, reserved = zero)]
pub struct Flags {
    /// Whether the feature is enabled.
    #[wire(bit = 0)]
    pub enabled: bool,
    /// A three-bit operating mode.
    #[wire(bits = 1..=3)]
    pub mode: u8,
    /// The semantic top bit of the storage scalar.
    #[wire(bit = 15)]
    pub high: bool,
}

/// A packet containing one independently owned flags representation.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct Packet {
    /// Packet kind.
    pub kind: u8,
    /// Nominal flags.
    pub flags: Flags,
    /// Packet sequence.
    #[wire(le)]
    pub sequence: u16,
}

#[test]
fn nominal_views_decode_semantic_bits_and_compose_without_reparsing() {
    let input = [7, 0x80, 0x0b, 0x34, 0x12, 0xaa];
    let (packet, suffix) = Packet::view(&input).with_remainder().unwrap();

    assert_eq!(packet.as_bytes(), &input[..5]);
    assert_eq!(suffix, &[0xaa]);
    assert_eq!(packet.kind(), 7);
    assert_eq!(packet.sequence(), 0x1234);

    let flags = packet.flags();
    assert_eq!(flags.as_bytes(), &[0x80, 0x0b]);
    assert!(flags.enabled());
    assert_eq!(flags.mode(), 5);
    assert!(flags.high());
}

#[test]
fn reads_accept_reserved_bits_and_writes_canonicalize_them_to_zero() {
    let view = Flags::view(&[0x7f, 0xf1]).without_trailing().unwrap();
    assert!(view.enabled());
    assert_eq!(view.mode(), 0);
    assert!(!view.high());
    assert_eq!(view.as_bytes(), &[0x7f, 0xf1]);

    let plan = Flags {
        enabled: true,
        mode: 5,
        high: true,
    }
    .prepare()
    .unwrap();
    assert_eq!(plan.encoded_len(), 2);

    let mut output = [0xa5; 3];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[0x80, 0x0b]);
    assert_eq!(suffix, &mut [0xa5]);
}

#[test]
fn projection_and_capacity_failures_leave_output_unchanged() {
    let mut output = [0xa5; 2];
    let before = output;
    let error = Flags {
        enabled: false,
        mode: 8,
        high: false,
    }
    .build_into(&mut output)
    .unwrap_err();
    assert!(matches!(
        error,
        wire_repr::BuildIntoError::Prepare(FlagsEncodeError::FieldOutOfRange {
            field: "mode",
            value: 8,
            width: 3,
        })
    ));
    assert_eq!(output, before);

    let plan = Flags {
        enabled: true,
        mode: 0,
        high: false,
    }
    .prepare()
    .unwrap();
    let mut short = [0xa5; 1];
    let before = short;
    assert!(matches!(
        plan.commit_into(&mut short),
        Err(wire_repr::OutputTooShortError {
            required: 2,
            available: 1,
        })
    ));
    assert_eq!(short, before);
}

#[test]
fn fixed_bitfield_sequences_are_exact_size_iterators() {
    let bytes = [0x80, 0x0b, 0x00, 0x02];
    let mut flags = Flags::views(&bytes).unwrap();
    assert_eq!(flags.len(), 2);
    let first = flags.next().unwrap();
    assert!(first.enabled());
    assert_eq!(first.mode(), 5);
    assert!(first.high());
    let second = flags.next().unwrap();
    assert!(!second.enabled());
    assert_eq!(second.mode(), 1);
    assert!(!second.high());
    assert!(flags.next().is_none());
}

fn cursor_byte_sum(source: &impl ByteSourceCursor) -> u8 {
    source.bytes().fold(0, u8::wrapping_add)
}

/// A parent whose checksum includes the complete prepared bitfield representation.
#[derive(Wire)]
pub struct BitfieldCursorChecksum<'wire> {
    /// Sum of the selected bitfield's physical bytes.
    #[wire(computed = cursor_byte_sum(include(flags)))]
    pub checksum: Computed<u8>,
    /// Nominal bitfield selected by the checksum.
    pub flags: Flags,
    /// Retains the wire lifetime without adding bytes.
    #[wire(rest)]
    pub rest: &'wire [u8],
}

#[test]
fn computed_include_consumes_complete_bitfield_cursor() {
    let mut output = [0; 3];
    let (written, suffix) = BitfieldCursorChecksum::builder()
        .flags(Flags {
            enabled: true,
            mode: 0,
            high: true,
        })
        .rest(&[])
        .build_into(&mut output)
        .unwrap();

    assert_eq!(written.as_bytes(), &[0x81, 0x80, 0x01]);
    assert_eq!(suffix, &mut []);
    assert_eq!(
        BitfieldCursorChecksum::view(written.as_bytes())
            .without_trailing()
            .unwrap()
            .checksum(),
        0x81
    );
}
