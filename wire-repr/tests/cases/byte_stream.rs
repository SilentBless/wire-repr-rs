use core::convert::Infallible;
use core::num::NonZeroUsize;

use wire_repr::codec::{self, ByteSegment, ByteSink, ByteSource, ByteSourceCursor};
use wire_repr::{
    BeI16, BeI32, BeI64, BeI128, BeU16, BeU24, BeU32, BeU64, BeU128, FixedCodec, I8, LeI16, LeI32,
    LeI64, LeI128, LeU16, LeU24, LeU32, LeU64, LeU128, PrefixCodec, PrefixExtent, U8,
    U24RangeError,
};

fn completed_plan<'value, C>(value: C::Value<'value>) -> C::Plan<'value>
where
    C: FixedCodec<EncodeError = Infallible> + 'value,
{
    match C::plan(value) {
        Ok(plan) => plan,
        Err(error) => match error {},
    }
}

fn render_plan<const N: usize>(plan: impl ByteSource) -> [u8; N] {
    assert_eq!(plan.byte_len(), N);
    let mut output = [0xa5; N];
    plan.write_into(&mut output);
    output
}

struct ChunkedSource;

impl ByteSource for ChunkedSource {
    fn byte_len(&self) -> usize {
        6
    }

    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        sink.write(&[1, 2]);
        sink.fill(0xa5, 3);
        sink.write(&[9]);
    }
}

struct RecordingSink {
    output: [u8; 6],
    written: usize,
    fill_calls: usize,
}

impl ByteSink for RecordingSink {
    fn write(&mut self, bytes: &[u8]) {
        let end = self.written + bytes.len();
        self.output[self.written..end].copy_from_slice(bytes);
        self.written = end;
    }

    fn fill(&mut self, byte: u8, len: usize) {
        let end = self.written + len;
        self.output[self.written..end].fill(byte);
        self.written = end;
        self.fill_calls += 1;
    }
}

struct UnderEmittingSource;

impl ByteSource for UnderEmittingSource {
    fn byte_len(&self) -> usize {
        2
    }

    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        sink.write(&[1]);
    }
}

struct OverEmittingSource;

impl ByteSource for OverEmittingSource {
    fn byte_len(&self) -> usize {
        1
    }

    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        sink.write(&[1, 2]);
    }
}

struct BorrowedValue<'wire>(&'wire [u8]);
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
}

struct Borrowing;
impl FixedCodec for Borrowing {
    type Value<'wire>
        = BorrowedValue<'wire>
    where
        Self: 'wire;
    type EncodeError = Infallible;
    type Plan<'value>
        = BorrowedPlan<'value>
    where
        Self: 'value;
    const WIDTH: usize = 2;
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        BorrowedValue(bytes)
    }
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok(BorrowedPlan(value.0))
    }
}
struct Marker;
impl FixedCodec for Marker {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;
    const WIDTH: usize = 1;
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        bytes[0]
    }
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([value])
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TinyDecodeError {
    Empty,
    Incomplete,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TinyEncodeError {
    ReservedMarker,
}
struct TinyPrefix;
impl PrefixCodec for TinyPrefix {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type DecodeError = TinyDecodeError;
    type EncodeError = TinyEncodeError;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;
    fn validate_prefix(bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        match bytes {
            [] => Err(TinyDecodeError::Empty),
            [0] => Err(TinyDecodeError::Incomplete),
            [0, _, ..] => Ok(PrefixExtent::new(NonZeroUsize::MIN.saturating_add(1))),
            [_, ..] => Ok(PrefixExtent::new(NonZeroUsize::MIN)),
        }
    }
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        if bytes[0] == 0 {
            bytes[1]
        } else {
            bytes[0] - 1
        }
    }
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        match value.checked_add(1) {
            Some(encoded) => Ok([encoded]),
            None => Err(TinyEncodeError::ReservedMarker),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminatedDecodeError {
    Incomplete,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminatedEncodeError {
    EmbeddedTerminator,
    LengthOverflow,
}
struct TerminatedPlan<'value> {
    value: &'value [u8],
    encoded_len: usize,
}
impl ByteSource for TerminatedPlan<'_> {
    fn byte_len(&self) -> usize {
        self.encoded_len
    }

    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        sink.write(self.value);
        sink.fill(0, 1);
    }
}

impl ByteSourceCursor for TerminatedPlan<'_> {
    type Segments<'source>
        = core::array::IntoIter<ByteSegment<'source>, 2>
    where
        Self: 'source;

    fn segments(&self) -> Self::Segments<'_> {
        [
            ByteSegment::Bytes(self.value),
            ByteSegment::Rest { byte: 0, len: 1 },
        ]
        .into_iter()
    }
}

struct Terminated;
impl PrefixCodec for Terminated {
    type Value<'wire>
        = &'wire [u8]
    where
        Self: 'wire;
    type DecodeError = TerminatedDecodeError;
    type EncodeError = TerminatedEncodeError;
    type Plan<'value>
        = TerminatedPlan<'value>
    where
        Self: 'value;
    fn validate_prefix(bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        match bytes.iter().position(|byte| *byte == 0) {
            Some(value_len) => match value_len.checked_add(1).and_then(NonZeroUsize::new) {
                Some(encoded_len) => Ok(PrefixExtent::new(encoded_len)),
                None => Err(TerminatedDecodeError::Incomplete),
            },
            None => Err(TerminatedDecodeError::Incomplete),
        }
    }
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        &bytes[..bytes.len() - 1]
    }
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        if value.contains(&0) {
            Err(TerminatedEncodeError::EmbeddedTerminator)
        } else {
            match value.len().checked_add(1) {
                Some(encoded_len) => Ok(TerminatedPlan { value, encoded_len }),
                None => Err(TerminatedEncodeError::LengthOverflow),
            }
        }
    }
}

#[path = "byte_stream/codecs.rs"]
mod codecs;
#[path = "byte_stream/prefix.rs"]
mod prefix;
#[path = "byte_stream/source.rs"]
mod source;
