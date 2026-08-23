#![deny(missing_docs, unsafe_code)]
//! Self-delimiting field coverage for `#[derive(Wire)]`.

use core::num::NonZeroUsize;
use wire_repr::{
    ByteSegment, ByteSink, ByteSource, ByteSourceCursor, PrefixCodec, PrefixExtent, PreparedLayout,
    Wire,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TinyDecodeError {
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TinyEncodeError {
    TooLarge,
}

struct TinyPlan {
    bytes: [u8; 2],
    len: usize,
}

impl ByteSource for TinyPlan {
    fn byte_len(&self) -> usize {
        self.len
    }

    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        sink.write(&self.bytes[..self.len]);
    }
}

impl ByteSourceCursor for TinyPlan {
    type Segments<'source>
        = core::iter::Once<ByteSegment<'source>>
    where
        Self: 'source;

    fn segments(&self) -> Self::Segments<'_> {
        core::iter::once(ByteSegment::Bytes(&self.bytes[..self.len]))
    }

    type Bytes<'source>
        = wire_repr::ByteBytes<'source, Self::Segments<'source>>
    where
        Self: 'source;

    fn bytes(&self) -> Self::Bytes<'_> {
        wire_repr::ByteBytes::new(self.segments())
    }
}

struct TinyPrefix;

impl PrefixCodec for TinyPrefix {
    type Value<'wire> = u16;
    type DecodeError = TinyDecodeError;
    type EncodeError = TinyEncodeError;
    type Plan<'value> = TinyPlan;

    fn validate_prefix(bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        let Some(first) = bytes.first() else {
            return Err(TinyDecodeError::Rejected);
        };
        if *first == 0xff {
            return Err(TinyDecodeError::Rejected);
        }
        let len = if first & 0x80 == 0 { 1 } else { 2 };
        Ok(PrefixExtent::new(NonZeroUsize::new(len).unwrap()))
    }

    fn decode(bytes: &[u8]) -> Self::Value<'_> {
        if bytes.len() == 1 {
            u16::from(bytes[0])
        } else {
            u16::from(bytes[1])
        }
    }

    fn plan(value: Self::Value<'_>) -> Result<Self::Plan<'_>, Self::EncodeError> {
        let value = u8::try_from(value).map_err(|_| TinyEncodeError::TooLarge)?;
        Ok(TinyPlan {
            bytes: [value, 0],
            len: 1,
        })
    }
}

#[derive(Debug, Eq, PartialEq, Wire)]
struct Packet {
    kind: u8,
    #[wire(prefix = TinyPrefix)]
    value: u16,
    tail: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BorrowedPlan<'value>(&'value [u8]);

impl ByteSource for BorrowedPlan<'_> {
    fn byte_len(&self) -> usize {
        self.0.len()
    }

    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        sink.write(self.0);
    }
}

impl ByteSourceCursor for BorrowedPlan<'_> {
    type Segments<'source>
        = core::iter::Once<ByteSegment<'source>>
    where
        Self: 'source;

    fn segments(&self) -> Self::Segments<'_> {
        core::iter::once(ByteSegment::Bytes(self.0))
    }

    type Bytes<'source>
        = wire_repr::ByteBytes<'source, Self::Segments<'source>>
    where
        Self: 'source;

    fn bytes(&self) -> Self::Bytes<'_> {
        wire_repr::ByteBytes::new(self.segments())
    }
}

struct BorrowedPrefix;

impl PrefixCodec for BorrowedPrefix {
    type Value<'wire> = &'wire [u8];
    type DecodeError = ();
    type EncodeError = ();
    type Plan<'value> = BorrowedPlan<'value>;

    fn validate_prefix(bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        let len = bytes
            .iter()
            .position(|byte| *byte == 0)
            .map(|index| index + 1)
            .ok_or(())?;
        Ok(PrefixExtent::new(NonZeroUsize::new(len).unwrap()))
    }

    fn decode(bytes: &[u8]) -> Self::Value<'_> {
        bytes
    }

    fn plan(value: Self::Value<'_>) -> Result<Self::Plan<'_>, Self::EncodeError> {
        if value.last() == Some(&0) {
            Ok(BorrowedPlan(value))
        } else {
            Err(())
        }
    }
}

#[derive(Debug, Eq, PartialEq, Wire)]
struct BorrowedPacket<'wire> {
    #[wire(prefix = BorrowedPrefix)]
    value: &'wire [u8],
    tail: u8,
}

#[test]
fn noncanonical_prefixes_decode_once_and_preserve_exact_framing() {
    let input = [3, 0x80, 7, 9, 0xaa];
    let (packet, suffix) = Packet::view(&input).with_remainder().unwrap();
    assert_eq!(packet.value(), 7);
    assert_eq!(packet.tail(), 9);
    assert_eq!(packet.as_bytes(), &input[..4]);
    assert_eq!(suffix, &[0xaa]);

    let mut output = [0xa5; 5];
    let plan = Packet {
        kind: packet.kind(),
        value: packet.value(),
        tail: packet.tail(),
    }
    .prepare()
    .unwrap();
    assert_eq!(plan.encoded_len(), 3);
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[3, 7, 9]);
    assert_eq!(suffix, &mut [0xa5, 0xa5]);
}

#[test]
fn validation_overclaim_and_planning_fail_before_output_mutation() {
    assert!(matches!(
        Packet::view(&[3, 0xff]).with_remainder(),
        Err(PacketDecodeError::Value(TinyDecodeError::Rejected))
    ));
    assert!(matches!(
        Packet::view(&[3, 0x80]).with_remainder(),
        Err(PacketDecodeError::InputTooShort {
            field: "value",
            required: 2,
            available: 1,
        })
    ));
    assert!(matches!(
        Packet {
            kind: 3,
            value: 256,
            tail: 9,
        }
        .prepare(),
        Err(PacketEncodeError::Value(TinyEncodeError::TooLarge))
    ));

    let plan = Packet {
        kind: 3,
        value: 7,
        tail: 9,
    }
    .prepare()
    .unwrap();
    let mut short = [0xa5; 2];
    assert!(plan.commit_into(&mut short).is_err());
    assert_eq!(short, [0xa5; 2]);
}

#[test]
fn borrowed_prefix_values_retain_the_wire_lifetime() {
    let input = [1, 2, 0, 9, 0xaa];
    let (packet, suffix) = BorrowedPacket::view(&input).with_remainder().unwrap();
    assert_eq!(packet.value(), &input[..3]);
    assert_eq!(packet.value().as_ptr(), input.as_ptr());
    assert_eq!(packet.tail(), 9);
    assert_eq!(suffix, &[0xaa]);

    let plan = BorrowedPacket {
        value: packet.value(),
        tail: packet.tail(),
    }
    .prepare()
    .unwrap();
    let mut output = [0xa5; 5];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &input[..4]);
    assert_eq!(suffix, &mut [0xa5]);
}
