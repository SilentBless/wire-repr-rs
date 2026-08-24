#![deny(missing_docs, unsafe_code)]
//! Declarative padding and alignment coverage.

use wire_repr::{ByteSink, ByteSource, PreparedLayout, Wire};

#[derive(Debug, Eq, PartialEq)]
enum SinkEvent {
    Write(Vec<u8>),
    Fill { byte: u8, len: usize },
}

#[derive(Default)]
struct RecordingSink {
    events: Vec<SinkEvent>,
}

impl ByteSink for RecordingSink {
    fn write(&mut self, bytes: &[u8]) {
        self.events.push(SinkEvent::Write(bytes.to_vec()));
    }

    fn fill(&mut self, byte: u8, len: usize) {
        self.events.push(SinkEvent::Fill { byte, len });
    }
}

/// A fixed representation with canonical padding before an aligned field.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct Padded {
    /// Leading byte.
    pub head: u8,
    /// Network-order value placed after padding and alignment.
    #[wire(be, pad_before = 2, align_before = 4)]
    pub tail: u16,
}

/// Alignment whose width depends on a preceding payload.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct DynamicAligned<'wire> {
    /// Encoded payload length.
    pub length: u8,
    /// Borrowed payload.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
    /// Marker aligned to a four-byte boundary.
    #[wire(align_before = 4)]
    pub tail: u8,
}

#[test]
fn decoding_accepts_opaque_padding_and_preserves_exact_framing() {
    let input = [7, 0xaa, 0xbb, 0xcc, 0x12, 0x34];
    let parsed = Padded::view(&input).unwrap();
    assert_eq!(parsed.head(), 7);
    assert_eq!(parsed.tail(), 0x1234);
    assert_eq!(parsed.as_bytes(), &input);

    let error = match Padded::view(&[7]) {
        Err(error) => error,
        Ok(_) => panic!("short padded input must be rejected"),
    };
    assert!(matches!(
        error,
        PaddedDecodeError::InputTooShort {
            field: "tail",
            required: 3,
            available: 0,
        }
    ));
}

#[test]
fn encoding_canonicalizes_padding_to_zero_atomically() {
    let plan = Padded {
        head: 7,
        tail: 0x1234,
    }
    .prepare()
    .unwrap();
    assert_eq!(plan.encoded_len(), 6);

    let mut sink = RecordingSink::default();
    plan.emit_to(&mut sink);
    assert_eq!(
        sink.events,
        [
            SinkEvent::Write(vec![7]),
            SinkEvent::Fill { byte: 0, len: 3 },
            SinkEvent::Write(vec![0x12, 0x34]),
        ]
    );

    let mut output = [0xa5; 8];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[7, 0, 0, 0, 0x12, 0x34]);
    assert_eq!(suffix, &mut [0xa5, 0xa5]);

    let plan = Padded {
        head: 7,
        tail: 0x1234,
    }
    .prepare()
    .unwrap();
    let mut short = [0xa5; 5];
    assert!(plan.commit_into(&mut short).is_err());
    assert_eq!(short, [0xa5; 5]);
}

#[test]
fn dynamic_alignment_is_resolved_during_preparation() {
    let payload = [1, 2];
    let plan = DynamicAligned {
        length: 99,
        payload: &payload,
        tail: 8,
    }
    .prepare()
    .unwrap();
    assert_eq!(plan.encoded_len(), 5);
    let mut output = [0xa5; 5];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[2, 1, 2, 0, 8]);
    assert!(suffix.is_empty());

    let payload = [1, 2, 3];
    let plan = DynamicAligned {
        length: 0,
        payload: &payload,
        tail: 9,
    }
    .prepare()
    .unwrap();
    let mut output = [0_u8; 5];
    let (written, _) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[3, 1, 2, 3, 9]);
}
