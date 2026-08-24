#![deny(missing_docs, unsafe_code)]
//! Static tagged enum derive coverage.

use wire_repr::{
    ByteSegment, ByteSink, ByteSource, ByteSourceCursor, FixedCodec, PreparedLayout, Wire,
};

#[derive(Default)]
struct RecordingSink {
    writes: Vec<Vec<u8>>,
}

impl ByteSink for RecordingSink {
    fn write(&mut self, bytes: &[u8]) {
        self.writes.push(bytes.to_vec());
    }

    fn fill(&mut self, _byte: u8, _len: usize) {}
}

/// A two-segment fixed tag representation used to exercise prepared enum cursors.
pub struct FragmentedTag;

/// The prepared fragmented tag, retaining separate physical tag spans.
pub struct FragmentedTagPlan {
    first: [u8; 1],
    second: [u8; 1],
}

impl ByteSource for FragmentedTagPlan {
    fn byte_len(&self) -> usize {
        2
    }

    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        sink.write(&self.first);
        sink.write(&self.second);
    }
}

impl ByteSourceCursor for FragmentedTagPlan {
    type Segments<'source>
        = core::array::IntoIter<ByteSegment<'source>, 2>
    where
        Self: 'source;

    fn segments(&self) -> Self::Segments<'_> {
        [
            ByteSegment::Bytes(&self.first),
            ByteSegment::Bytes(&self.second),
        ]
        .into_iter()
    }

    type Bytes<'source>
        = wire_repr::ByteBytes<'source, Self::Segments<'source>>
    where
        Self: 'source;

    fn bytes(&self) -> Self::Bytes<'_> {
        wire_repr::ByteBytes::new(self.segments())
    }
}

impl FixedCodec for FragmentedTag {
    type Value<'wire> = u8;
    type EncodeError = core::convert::Infallible;
    type Plan<'value> = FragmentedTagPlan;

    const WIDTH: usize = 2;

    fn decode(bytes: &[u8]) -> Self::Value<'_> {
        bytes[0]
    }

    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok(FragmentedTagPlan {
            first: [value],
            second: [0xfe],
        })
    }
}

/// Semantic ping-body validation failure.
#[derive(Debug)]
pub enum PingError {
    /// Structural ping decoding failed.
    Decode(PingDecodeError),
    /// The ping value must not be zero.
    Zero,
}

impl From<PingDecodeError> for PingError {
    fn from(error: PingDecodeError) -> Self {
        Self::Decode(error)
    }
}

impl core::fmt::Display for PingError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("ping value must not be zero")
    }
}

impl core::error::Error for PingError {}

fn ping_nonzero(value: u16) -> Result<(), PingError> {
    if value == 0 {
        Err(PingError::Zero)
    } else {
        Ok(())
    }
}

/// A fixed body carried by one operation variant.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(error = PingError)]
pub struct Ping {
    /// Network-order ping value.
    #[wire(be, validate = ping_nonzero)]
    pub value: u16,
}

/// An inline tagged operation.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(tag = U8)]
#[wire(unknown = reject)]
#[repr(u8)]
pub enum Operation {
    /// Carries a fixed ping body.
    Ping(Ping) = 1,
    /// Carries no body.
    Halt = 2,
}

/// An enum with a custom tag plan split across two physical segments.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(tag = FragmentedTag, unknown = reject)]
#[repr(u8)]
pub enum FragmentedOperation {
    /// Carries a fixed ping body after the fragmented tag.
    Ping(Ping) = 2,
    /// Carries no body after the fragmented tag.
    Halt = 3,
}

/// Two independent tagged selections between fixed siblings.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct Packet {
    /// Leading fixed byte.
    pub lead: u8,
    /// First tagged selection.
    pub first: Operation,
    /// Fixed separator.
    pub separator: u8,
    /// Second tagged selection.
    pub second: Operation,
    /// Trailing fixed byte.
    pub tail: u8,
}

/// Semantic opcode identities owned by the consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Opcode {
    /// Ping operation.
    Ping,
    /// Halt operation.
    Halt,
}

/// Consumer-owned opcode mapping failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpcodeMapError;

impl core::fmt::Display for OpcodeMapError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("opcode table unavailable")
    }
}

impl core::error::Error for OpcodeMapError {}

/// Consumer-owned bidirectional opcode table.
pub struct Opcodes {
    ping: u8,
    halt: u8,
    fail: bool,
}

impl Opcodes {
    fn decode(&self, raw: u8) -> Result<Option<Opcode>, OpcodeMapError> {
        if self.fail {
            return Err(OpcodeMapError);
        }
        Ok(if raw == self.ping {
            Some(Opcode::Ping)
        } else if raw == self.halt {
            Some(Opcode::Halt)
        } else {
            None
        })
    }

    fn encode(&self, opcode: Opcode) -> Result<Option<u8>, OpcodeMapError> {
        if self.fail {
            return Err(OpcodeMapError);
        }
        Ok(Some(match opcode {
            Opcode::Ping => self.ping,
            Opcode::Halt => self.halt,
        }))
    }
}

/// An operation dispatched through a consumer-owned opcode table.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(
    tag = U8,
    opcodes = Opcodes,
    opcodes_error = OpcodeMapError,
    unknown = reject
)]
pub enum MappedOperation {
    /// Carries a fixed ping body.
    #[wire(opcodes = Opcode::Ping)]
    Ping(Ping),
    /// Carries no body.
    #[wire(opcodes = Opcode::Halt)]
    Halt,
}

/// A table-named operation mapping, deliberately unrelated to the legacy `opcodes` spelling.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(tag = U8, table = Opcodes, table_error = OpcodeMapError, unknown = reject)]
pub enum TableOperation {
    /// Carries a fixed ping body.
    #[wire(table = Opcode::Ping)]
    Ping(Ping),
    /// Carries no body.
    #[wire(table = Opcode::Halt)]
    Halt,
}

/// A table-selected tag-only enum.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(tag = U8, table = Opcodes, table_error = OpcodeMapError, unknown = reject)]
pub enum TableSignal {
    /// Carries no body.
    #[wire(table = Opcode::Halt)]
    Halt,
}

/// A packet sharing one opcode mapping across independent selections.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(opcodes = Opcodes)]
pub struct MappedPacket {
    /// Leading marker.
    pub lead: u8,
    /// First mapped operation.
    #[wire(opcodes)]
    pub first: MappedOperation,
    /// Separator marker.
    pub separator: u8,
    /// Second mapped operation.
    #[wire(opcodes)]
    pub second: MappedOperation,
    /// Trailing marker.
    pub tail: u8,
}

/// A struct forwarding a schema-named table to two independent fields.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(table = Opcodes)]
pub struct TablePacket {
    /// First mapped operation.
    #[wire(table)]
    pub first: TableOperation,
    /// Second mapped operation.
    #[wire(table)]
    pub second: TableOperation,
}

/// A borrowed packet forwarding a table into a tag-only child.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(table = Opcodes)]
pub struct BorrowedTablePacket<'wire> {
    /// Selected signal.
    #[wire(table)]
    pub signal: TableSignal,
    /// Remaining borrowed payload.
    #[wire(rest)]
    pub payload: &'wire [u8],
}

/// A wide big-endian tag.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(tag = BeU16)]
#[wire(unknown = reject)]
#[repr(u16)]
pub enum WideOperation {
    /// Bodyless wide-tag case.
    Halt = 0x1234,
}

/// A tagged body borrowing a bounded byte slice.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct BorrowedBody<'wire> {
    /// Encoded payload length.
    pub length: u8,
    /// Borrowed payload.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
}

/// A table-selected enum whose semantic body carries a borrow.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(tag = U8, table = Opcodes, table_error = OpcodeMapError, unknown = reject)]
pub enum BorrowedTableOperation<'wire> {
    /// Carries a borrowed body.
    #[wire(table = Opcode::Ping)]
    Ping(BorrowedBody<'wire>),
}

/// A borrowed parent forwarding its table to a borrowed enum body.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(table = Opcodes)]
pub struct BorrowedTableEnvelope<'wire> {
    /// Selected borrowed operation.
    #[wire(table)]
    pub operation: BorrowedTableOperation<'wire>,
}

/// A borrowed forwarding struct whose plan does not otherwise need its semantic lifetime.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(table = Opcodes)]
pub struct BorrowedTableChild<'wire> {
    /// Selected bodyless operation.
    #[wire(table)]
    pub signal: TableSignal,
    /// Remaining borrowed payload.
    #[wire(rest)]
    pub payload: &'wire [u8],
}

/// A borrowed parent forwarding through another dynamic struct.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(table = Opcodes)]
pub struct BorrowedTableParent<'wire> {
    /// Nested forwarding child.
    #[wire(table)]
    pub child: BorrowedTableChild<'wire>,
}

/// Human-owned validation error for a forwarding struct.
#[derive(Debug, Eq, PartialEq)]
pub enum CustomTableEnvelopeError {
    /// Parent framing failed.
    Decode,
    /// Nested signal validation failed.
    Signal,
}

impl core::fmt::Display for CustomTableEnvelopeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl core::error::Error for CustomTableEnvelopeError {}

impl From<CustomTableEnvelopeDecodeError<'_>> for CustomTableEnvelopeError {
    fn from(_: CustomTableEnvelopeDecodeError<'_>) -> Self {
        Self::Decode
    }
}

impl From<TableSignalDecodeError<'_>> for CustomTableEnvelopeError {
    fn from(_: TableSignalDecodeError<'_>) -> Self {
        Self::Signal
    }
}

/// A forwarding struct selecting a human-owned validation error.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(table = Opcodes, error = CustomTableEnvelopeError)]
pub struct CustomTableEnvelope {
    /// Selected bodyless operation.
    #[wire(table)]
    pub signal: TableSignal,
}

/// A tagged semantic enum retaining its input lifetime.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(tag = U8)]
#[wire(unknown = reject)]
#[repr(u8)]
pub enum BorrowedOperation<'value> {
    /// Carries a borrowed body.
    Data(BorrowedBody<'value>) = 1,
    /// Carries no body.
    Halt = 2,
}

/// A closed enum selected by exact fixed-width byte tags.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(tag = [u8; 4], unknown = reject)]
pub enum ByteOperation {
    /// Carries no body.
    #[wire(tag = b"HALT")]
    Halt,
    /// Carries a fixed ping body.
    #[wire(tag = b"PING")]
    Ping(Ping),
}

/// An open fixed-byte enum preserving unknown tags losslessly.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(tag = [u8; 4], unknown = preserve)]
pub enum OpenByteOperation {
    /// One known selector.
    #[wire(tag = b"HALT")]
    Halt,
    /// Any undeclared raw selector.
    #[wire(unknown)]
    Other([u8; 4]),
}

/// An open integer enum preserving unknown tags without reinterpretation.
#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(tag = U8, unknown = preserve)]
#[repr(u8)]
pub enum OpenIntegerOperation {
    /// One known selector.
    Halt = 1,
    /// Any undeclared raw selector.
    #[wire(unknown)]
    Other(u8),
}

fn cursor_byte_sum(source: &impl ByteSourceCursor) -> u8 {
    source.bytes().fold(0, u8::wrapping_add)
}

/// A parent whose checksum includes the complete prepared enum representation.
#[derive(Wire)]
pub struct EnumCursorChecksum<'wire> {
    /// Sum of the selected enum's physical bytes.
    #[wire(computed = cursor_byte_sum(include(operation)))]
    pub checksum: u8,
    /// Tagged operation selected by the checksum.
    pub operation: Operation,
    /// Retains the wire lifetime without adding bytes.
    #[wire(rest)]
    pub rest: &'wire [u8],
}

/// A parent whose checksum consumes a fragmented prepared enum tag before its body.
#[derive(Wire)]
pub struct FragmentedEnumCursorChecksum<'wire> {
    /// Sum of the selected enum's physical bytes.
    #[wire(computed = cursor_byte_sum(include(operation)))]
    pub checksum: u8,
    /// Tagged operation selected by the checksum.
    pub operation: FragmentedOperation,
    /// Retains the wire lifetime without adding bytes.
    #[wire(rest)]
    pub rest: &'wire [u8],
}

#[path = "tagged_frame/direct.rs"]
mod direct;
#[path = "tagged_frame/mapping.rs"]
mod mapping;
#[path = "tagged_frame/validation.rs"]
mod validation;
