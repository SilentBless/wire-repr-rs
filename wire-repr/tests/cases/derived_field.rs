#![deny(missing_docs, unsafe_code)]
//! Computed byte-length field coverage.

use wire_repr::{ByteSourceCursor, FixedCodec, PreparedLayout, Wire};

/// A packet with a bounded payload length.
#[derive(Wire)]
pub struct Packet<'wire> {
    /// Payload byte count derived from the payload extent.
    pub length: u8,
    /// Borrowed payload.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
    /// Packet kind.
    pub kind: u8,
}

/// A big-endian derived 16-bit length.
#[derive(Wire)]
pub struct BigEndianPacket<'wire> {
    /// Payload byte count derived from the payload extent.
    #[wire(be)]
    pub length: u16,
    /// Borrowed payload.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
}

/// A little-endian derived 16-bit length.
#[derive(Wire)]
pub struct LittleEndianPacket<'wire> {
    /// Payload byte count derived from the payload extent.
    #[wire(le)]
    pub length: u16,
    /// Borrowed payload.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
}

/// A computed semantic length with an explicitly selected 24-bit representation.
#[derive(Wire)]
pub struct Qualified<'wire> {
    /// Payload byte count.
    #[wire(computed = wire_repr::computation::len(payload), codec = wire_repr::BeU24)]
    pub length: u32,
    /// Remaining payload bytes.
    #[wire(rest)]
    pub payload: &'wire [u8],
}

/// A nominal payload length with an explicit one-byte representation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PayloadLength(u8);

impl TryFrom<usize> for PayloadLength {
    type Error = core::num::TryFromIntError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        Ok(Self(u8::try_from(value)?))
    }
}

impl From<PayloadLength> for usize {
    fn from(value: PayloadLength) -> Self {
        usize::from(value.0)
    }
}

/// A fixed codec preserving the nominal length type.
pub struct PayloadLengthCodec;

impl FixedCodec for PayloadLengthCodec {
    type Value<'wire> = PayloadLength;
    type EncodeError = core::convert::Infallible;
    type Plan<'value> = [u8; 1];

    const WIDTH: usize = 1;

    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        PayloadLength(bytes[0])
    }

    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([value.0])
    }
}

/// A computed semantic length whose value is a nominal type.
#[derive(Wire)]
pub struct NominalPacket<'wire> {
    /// Payload byte count.
    #[wire(computed = wire_repr::computation::len(payload), codec = PayloadLengthCodec)]
    pub length: PayloadLength,
    /// Remaining payload bytes.
    #[wire(rest)]
    pub payload: &'wire [u8],
}

/// A packet whose remainder is length-prefixed.
#[derive(Wire)]
pub struct RestPacket<'wire> {
    /// Remainder byte count.
    #[wire(computed = wire_repr::computation::len(payload))]
    pub length: u8,
    /// Remaining bytes.
    #[wire(rest)]
    pub payload: &'wire [u8],
}

/// A borrowed nested tail used by a computed parent builder.
#[derive(Wire)]
pub struct NestedTail<'wire> {
    /// Tail kind.
    pub kind: u8,
    /// Remaining tail bytes.
    #[wire(rest)]
    pub data: &'wire [u8],
}

/// A bounded parent retaining a borrowed nested semantic input.
#[derive(Wire)]
pub struct NestedPacket<'wire> {
    /// Payload byte count derived from the payload extent.
    pub length: u8,
    /// Borrowed payload.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
    /// Borrowed nested tail.
    pub tail: NestedTail<'wire>,
}

fn byte_sum(source: &impl ByteSourceCursor) -> u8 {
    source.bytes().fold(0u8, u8::wrapping_add)
}

fn byte_count(source: &impl ByteSourceCursor) -> usize {
    source.byte_len()
}

fn oversized_count(_: &impl ByteSourceCursor) -> usize {
    256
}

fn constant_count() -> usize {
    7
}

fn semantic_count(value: &u8) -> usize {
    usize::from(*value) + 1
}

fn ordered_count(
    kind: &u8,
    first: &impl ByteSourceCursor,
    remaining: &impl ByteSourceCursor,
) -> usize {
    usize::from(*kind) * 100 + first.byte_len() * 10 + remaining.byte_len()
}

/// A computation receiving a semantic field by shared reference.
#[derive(Wire)]
pub struct SemanticCallback {
    /// Derived from the semantic value.
    #[wire(computed = semantic_count(value))]
    pub count: u8,
    /// Ordinary semantic input.
    pub value: u8,
}

/// A computation with no callback arguments.
#[derive(Wire)]
pub struct NoArgumentCallback {
    /// Constant computed value.
    #[wire(computed = constant_count())]
    pub count: u8,
}

/// An empty physical selection remains an ordinary callback argument.
#[derive(Wire)]
pub struct EmptyIncludeCallback {
    /// Number of selected bytes.
    #[wire(computed = byte_count(include()))]
    pub count: u8,
    /// Ordinary field omitted from the empty selection.
    pub value: u8,
}

/// An empty exclusion selects every available physical field.
#[derive(Wire)]
pub struct EmptyExcludeCallback {
    /// Number of available bytes.
    #[wire(computed = byte_count(exclude()))]
    pub count: u8,
    /// Ordinary field retained by the empty exclusion.
    pub value: u8,
}

/// A computation with semantic and independently selected physical arguments.
#[derive(Wire)]
pub struct OrderedCallback {
    /// Ordered callback result.
    #[wire(computed = ordered_count(kind, include(first), exclude(second)))]
    pub checksum: u8,
    /// Semantic callback input.
    pub kind: u8,
    /// First physical selection.
    pub first: u8,
    /// Explicitly excluded physical field.
    pub second: u8,
    /// Remaining physical bytes included by the exclusion selection.
    pub tail: u8,
}

/// Duplicate selectors are one physical set rather than an error.
#[derive(Wire)]
pub struct DuplicateSelection {
    /// Count of the selected bytes.
    #[wire(computed = byte_count(include(value, value)))]
    pub count: u8,
    /// One selected byte.
    pub value: u8,
}

/// A callback-derived count converted into its computed destination type.
#[derive(Wire)]
pub struct CallbackCount {
    /// Number of selected physical bytes.
    #[wire(computed = byte_count(include(value)))]
    pub count: u8,
    /// The selected byte.
    pub value: u8,
}

/// A callback whose result is not representable by its destination type.
#[derive(Wire)]
pub struct OversizedCallback {
    /// Callback result converted into the computed destination type.
    #[wire(computed = oversized_count(include(value)))]
    pub count: u8,
    /// The selected byte.
    pub value: u8,
}

/// A fixed-only representation with a callback over owned fields.
#[derive(Wire)]
pub struct FixedCallback {
    /// Sum of the selected physical bytes.
    #[wire(computed = byte_sum(include(first, second)))]
    pub checksum: u8,
    /// First selected byte.
    pub first: u8,
    /// Second selected byte.
    pub second: u8,
}

/// A representation containing only a self-excluding computed field.
#[derive(Wire)]
pub struct FixedExcludeSelf {
    /// Sum of all physical bytes except this field.
    #[wire(computed = byte_sum(exclude(self)))]
    pub checksum: u8,
}

/// A checksum that depends on a later computed length field.
#[derive(Wire)]
pub struct Checksummed<'wire> {
    /// Sum of every physical byte except this field.
    #[wire(computed = byte_sum(exclude(self)))]
    pub checksum: u8,
    /// Payload byte count.
    #[wire(computed = wire_repr::computation::len(payload))]
    pub length: u8,
    /// Remaining payload bytes.
    #[wire(rest)]
    pub payload: &'wire [u8],
}

/// Computed fields whose physical read-set determines preparation order.
#[derive(Wire)]
pub struct IncludedDependency<'wire> {
    /// Sum of the prepared partial checksum and payload.
    #[wire(computed = byte_sum(include(partial, payload)))]
    pub checksum: u8,
    /// Sum of the payload alone.
    #[wire(computed = byte_sum(include(payload)))]
    pub partial: u8,
    /// Remaining payload bytes.
    #[wire(rest)]
    pub payload: &'wire [u8],
}

/// A checksum selecting one projected nested field.
#[derive(Wire)]
pub struct NestedSelection<'wire> {
    /// Sum of the nested kind byte.
    #[wire(computed = byte_sum(include(tail.kind)))]
    pub checksum: u8,
    /// Nested representation.
    pub tail: NestedTail<'wire>,
}

fn source_len(source: &impl ByteSourceCursor) -> u8 {
    u8::try_from(source.byte_len()).expect("test source length fits u8")
}

/// A computation whose selected source includes physical positioning gaps.
#[derive(Wire)]
pub struct PositionedChecksum<'wire> {
    /// Length of every physical byte except this field.
    #[wire(computed = source_len(exclude(self)))]
    pub checksum: u8,
    /// A positioned byte preceded by a generated gap.
    #[wire(at = 4)]
    pub marker: u8,
    /// Trailing byte.
    pub tail: u8,
    /// Remaining bytes.
    #[wire(rest)]
    pub payload: &'wire [u8],
}

#[derive(Debug)]
enum ChecksumError {
    Decode(ValidatedChecksumDecodeError),
    Mismatch,
}

impl core::fmt::Display for ChecksumError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Decode(error) => error.fmt(formatter),
            Self::Mismatch => formatter.write_str("checksum mismatch"),
        }
    }
}

impl core::error::Error for ChecksumError {}

impl From<ValidatedChecksumDecodeError> for ChecksumError {
    fn from(error: ValidatedChecksumDecodeError) -> Self {
        Self::Decode(error)
    }
}

fn checksum_matches(view: &ValidatedChecksumView<'_>) -> Result<(), ChecksumError> {
    let expected = byte_sum(&view.bytes().exclude(|fields| fields.checksum));
    if view.checksum() == expected {
        Ok(())
    } else {
        Err(ChecksumError::Mismatch)
    }
}

#[derive(Wire)]
#[wire(error = ChecksumError, validate = checksum_matches)]
#[allow(dead_code)]
struct ValidatedChecksum<'wire> {
    #[wire(computed = byte_sum(exclude(self)))]
    checksum: u8,
    #[wire(rest)]
    payload: &'wire [u8],
}

/// Consumer-owned callback selector.
#[derive(Clone, Copy, PartialEq)]
pub enum CallbackSelector {
    /// The only selected nested variant.
    Ping,
}

/// Consumer-owned callback table failure.
#[derive(Debug)]
pub struct CallbackTableError;

impl core::fmt::Display for CallbackTableError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("callback table unavailable")
    }
}

impl core::error::Error for CallbackTableError {}

/// A consumer-owned callback table.
pub struct CallbackTable {
    tag: u8,
}

impl CallbackTable {
    fn decode(&self, tag: u8) -> Result<Option<CallbackSelector>, CallbackTableError> {
        Ok((tag == self.tag).then_some(CallbackSelector::Ping))
    }

    fn encode(&self, selector: CallbackSelector) -> Result<Option<u8>, CallbackTableError> {
        match selector {
            CallbackSelector::Ping => Ok(Some(self.tag)),
        }
    }
}

/// A fixed nested callback body.
#[derive(Wire)]
pub struct CallbackBody {
    /// Body byte.
    pub value: u8,
}

/// An operation-table-selected nested enum.
#[derive(Wire)]
#[wire(tag = U8, table = CallbackTable, table_error = CallbackTableError, unknown = reject)]
pub enum CallbackOperation {
    /// Carries a fixed body.
    #[wire(table = CallbackSelector::Ping)]
    Ping(CallbackBody),
}

/// A fixed-only computed builder with a table-selected nested operation.
#[derive(Wire)]
#[wire(table = CallbackTable)]
pub struct CallbackEnvelope {
    /// Checksum over the selected operation's exact physical bytes.
    #[wire(computed = byte_sum(include(selected)))]
    pub checksum: u8,
    /// Selected operation.
    #[wire(table)]
    pub selected: CallbackOperation,
}

/// A borrowed bounded payload with a computed table-selected checksum.
#[derive(Wire)]
#[wire(table = CallbackTable)]
pub struct CallbackPayload<'wire> {
    /// Sum of the selected operation's physical bytes.
    #[wire(computed = byte_sum(include(selected)))]
    pub checksum: u8,
    /// Payload byte count derived from the payload extent.
    pub length: u8,
    /// Borrowed payload.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
    /// Selected operation.
    #[wire(table)]
    pub selected: CallbackOperation,
}

#[path = "derived_field/arguments.rs"]
mod arguments;
#[path = "derived_field/conversion.rs"]
mod conversion;
#[path = "derived_field/lifecycle.rs"]
mod lifecycle;
