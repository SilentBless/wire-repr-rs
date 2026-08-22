#![deny(missing_docs, unsafe_code)]
//! Static tagged enum derive coverage.

use wire_repr::{
    ByteSegment, ByteSink, ByteSource, ByteSourceCursor, Computed, FixedCodec, PreparedLayout, Wire,
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
    /// The ping value must not be zero.
    Zero,
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

#[test]
fn enum_framing_handles_unit_body_unknown_and_truncation() {
    let (halt, suffix) = Operation::view(&[2, 99]).with_remainder().unwrap();
    assert_eq!(halt.as_bytes(), &[2]);
    assert!(halt.is_halt());
    assert!(halt.ping().is_none());
    assert_eq!(suffix, &[99]);

    let ping = Operation::view(&[1, 0x12, 0x34])
        .without_trailing()
        .unwrap();
    assert_eq!(ping.as_bytes(), &[1, 0x12, 0x34]);
    assert!(!ping.is_halt());
    let body = ping.ping().unwrap();
    assert_eq!(body.value(), 0x1234);
    assert_eq!(body.as_bytes(), &[0x12, 0x34]);
    let copied = ping;
    assert_eq!(copied.ping().unwrap().as_bytes(), body.as_bytes());

    assert!(matches!(
        Operation::view(&[3]).without_trailing(),
        Err(OperationValidationError::Decode(
            OperationDecodeError::UnknownTag { tag: 3 }
        ))
    ));
    let error = Operation::view(&[]).without_trailing().unwrap_err();
    assert!(matches!(
        error,
        OperationValidationError::Decode(OperationDecodeError::InputTooShort {
            required: 1,
            available: 0,
        })
    ));
    assert_eq!(
        error.to_string(),
        "tag needs 1 byte, but only 0 bytes remain"
    );

    let error = Operation::view(&[3]).without_trailing().unwrap_err();
    assert!(matches!(
        error,
        OperationValidationError::Decode(OperationDecodeError::UnknownTag { tag: 3 })
    ));
    assert_eq!(error.to_string(), "unknown wire tag 3");

    let error = Operation::view(&[1, 0]).without_trailing().unwrap_err();
    assert!(matches!(
        error,
        OperationValidationError::Decode(OperationDecodeError::Ping(
            PingDecodeError::InputTooShort {
                field: "value",
                required: 2,
                available: 1,
            }
        ))
    ));
    assert_eq!(
        error.to_string(),
        "wire decode failed in variant `Ping`: field `value` needs 2 bytes, but only 1 byte remains"
    );

    let error = Operation::view(&[2, 99]).without_trailing().unwrap_err();
    assert!(matches!(
        error,
        OperationValidationError::Decode(OperationDecodeError::TrailingBytes {
            expected: 1,
            actual: 2,
        })
    ));
    assert_eq!(
        error.to_string(),
        "input has 1 trailing byte after the 1-byte representation"
    );
}

#[test]
fn nested_enum_fields_round_trip_and_short_output_is_atomic() {
    let packet = Packet {
        lead: 9,
        first: Operation::Ping(Ping { value: 0x1234 }),
        separator: 8,
        second: Operation::Halt,
        tail: 7,
    };
    let plan = packet.prepare().unwrap();
    assert_eq!(plan.encoded_len(), 7);

    let mut output = [0_u8; 9];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[9, 1, 0x12, 0x34, 8, 2, 7]);
    assert_eq!(suffix, &mut [0, 0]);

    let parsed = Packet::view(written.as_bytes()).without_trailing().unwrap();
    assert_eq!(parsed.as_bytes(), &[9, 1, 0x12, 0x34, 8, 2, 7]);
    assert_eq!(parsed.lead(), 9);
    assert_eq!(parsed.first().ping().unwrap().value(), 0x1234);
    assert_eq!(parsed.separator(), 8);
    assert!(parsed.second().is_halt());
    assert_eq!(parsed.tail(), 7);

    assert!(matches!(
        Packet::view(&[9, 3, 8, 2, 7]).without_trailing(),
        Err(PacketValidationError::Decode(PacketDecodeError::First(
            OperationDecodeError::UnknownTag { tag: 3 }
        )))
    ));

    let mut short = [0xa5; 6];
    let packet = Packet {
        lead: 9,
        first: Operation::Ping(Ping { value: 0x1234 }),
        separator: 8,
        second: Operation::Halt,
        tail: 7,
    };
    assert!(packet.build_into(&mut short).is_err());
    assert_eq!(short, [0xa5; 6]);
}

#[test]
fn enum_byte_source_emits_tag_before_selected_body() {
    let plan = Operation::Ping(Ping { value: 0x1234 }).prepare().unwrap();
    let mut sink = RecordingSink::default();
    plan.emit_to(&mut sink);
    assert_eq!(sink.writes, [vec![1], vec![0x12, 0x34]]);
}

#[test]
fn tag_codec_controls_wire_endianness() {
    assert_eq!(
        OperationEncodeError::Ping(PingEncodeError::LengthOverflow).to_string(),
        "wire preparation failed for variant `Ping`: encoded representation length does not fit in usize"
    );
    assert_eq!(
        OperationEncodeError::LengthOverflow.to_string(),
        "encoded representation length does not fit in usize"
    );

    let plan = WideOperation::Halt.prepare().unwrap();
    let mut output = [0_u8; 2];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();

    assert_eq!(written.as_bytes(), &[0x12, 0x34]);
    assert!(suffix.is_empty());
    let view = WideOperation::view(written.as_bytes())
        .without_trailing()
        .unwrap();
    assert!(view.is_halt());
    assert_eq!(view.as_bytes(), &[0x12, 0x34]);
}

#[test]
fn tagged_enums_retain_borrowed_variant_values() {
    let input = [1, 3, 7, 8, 9, 0xaa];
    let (parsed, suffix) = BorrowedOperation::view(&input).with_remainder().unwrap();
    assert_eq!(parsed.as_bytes(), &input[..5]);
    let body = parsed.data().expect("data tag should select the data body");
    assert_eq!(body.payload(), &input[2..5]);
    assert!(core::ptr::eq(body.payload().as_ptr(), input[2..5].as_ptr()));
    assert_eq!(body.as_bytes(), &input[1..5]);
    assert!(!parsed.is_halt());
    assert_eq!(suffix, &[0xaa]);

    let payload = [4, 5];
    let plan = BorrowedOperation::Data(BorrowedBody {
        length: 99,
        payload: &payload,
    })
    .prepare()
    .unwrap();
    let mut output = [0_u8; 5];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[1, 2, 4, 5]);
    assert_eq!(suffix, &mut [0]);
}

#[test]
fn runtime_opcode_mapping_is_bidirectional_and_explicit() {
    let opcodes = Opcodes {
        ping: 0x41,
        halt: 0x7f,
        fail: false,
    };

    let view = MappedOperation::view(&[0x41, 0x12, 0x34])
        .opcodes(&opcodes)
        .without_trailing()
        .unwrap();
    assert_eq!(view.as_bytes(), &[0x41, 0x12, 0x34]);
    assert_eq!(view.ping().unwrap().value(), 0x1234);

    assert!(matches!(
        MappedOperation::view(&[0x55])
            .opcodes(&opcodes)
            .without_trailing(),
        Err(MappedOperationValidationError::Decode(
            MappedOperationDecodeError::UnknownTag { tag: 0x55 }
        ))
    ));

    let failing = Opcodes {
        ping: 0x41,
        halt: 0x7f,
        fail: true,
    };
    assert!(matches!(
        MappedOperation::view(&[0x41])
            .opcodes(&failing)
            .without_trailing(),
        Err(MappedOperationValidationError::Decode(
            MappedOperationDecodeError::OperationMapping(OpcodeMapError)
        ))
    ));

    let plan = MappedOperation::Halt.opcodes(&opcodes).prepare().unwrap();
    let mut output = [0xa5; 2];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[0x7f]);
    assert_eq!(suffix, &mut [0xa5]);

    let mut short = [];
    assert!(
        MappedOperation::Halt
            .opcodes(&opcodes)
            .build_into(&mut short)
            .is_err()
    );
}

#[test]
fn one_opcode_input_composes_across_independent_enum_fields() {
    let opcodes = Opcodes {
        ping: 0x31,
        halt: 0x62,
        fail: false,
    };
    let input = [9, 0x31, 0x12, 0x34, 8, 0x62, 7, 0xaa];
    let (view, suffix) = MappedPacket::view(&input)
        .opcodes(&opcodes)
        .with_remainder()
        .unwrap();
    assert_eq!(view.as_bytes(), &input[..7]);
    assert_eq!(suffix, &[0xaa]);
    assert_eq!(view.first().ping().unwrap().value(), 0x1234);
    assert!(view.second().is_halt());

    let plan = MappedPacket {
        lead: 9,
        first: MappedOperation::Ping(Ping { value: 0x1234 }),
        separator: 8,
        second: MappedOperation::Halt,
        tail: 7,
    }
    .opcodes(&opcodes)
    .prepare()
    .unwrap();
    let mut output = [0xa5; 8];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &input[..7]);
    assert_eq!(suffix, &mut [0xa5]);

    let failing = Opcodes {
        ping: 0x31,
        halt: 0x62,
        fail: true,
    };
    assert!(matches!(
        MappedPacket::view(&input)
            .opcodes(&failing)
            .with_remainder(),
        Err(MappedPacketValidationError::Decode(
            MappedPacketDecodeError::First(MappedOperationDecodeError::OperationMapping(
                OpcodeMapError
            ))
        ))
    ));
    assert!(matches!(
        MappedPacket {
            lead: 9,
            first: MappedOperation::Ping(Ping { value: 0x1234 }),
            separator: 8,
            second: MappedOperation::Halt,
            tail: 7,
        }
        .opcodes(&failing)
        .prepare(),
        Err(MappedPacketEncodeError::First(
            MappedOperationEncodeError::OperationMapping(OpcodeMapError)
        ))
    ));
}

#[test]
fn opcode_validated_cursor_retains_the_failing_item() {
    let opcodes = Opcodes {
        ping: 0x31,
        halt: 0x62,
        fail: false,
    };
    let input = [9, 0x62, 8, 0x62, 7, 9, 0x55, 8, 0x62, 7];
    let mut cursor = MappedPacket::cursor(&input).opcodes(&opcodes);
    assert!(cursor.next().unwrap().is_some());
    assert_eq!(cursor.remaining(), &input[5..]);
    assert!(matches!(
        cursor.next(),
        Err(wire_repr::ViewCursorError::Item(
            MappedPacketValidationError::Decode(MappedPacketDecodeError::First(
                MappedOperationDecodeError::UnknownTag { tag: 0x55 }
            ))
        ))
    ));
    assert_eq!(cursor.remaining(), &input[5..]);
    let mut unchecked = cursor.unchecked();
    assert!(matches!(
        unchecked.next(),
        Err(wire_repr::ViewCursorError::Item(
            MappedPacketDecodeError::First(MappedOperationDecodeError::UnknownTag { tag: 0x55 })
        ))
    ));
    assert_eq!(unchecked.remaining(), &input[5..]);
}

#[test]
fn table_named_enum_requests_are_direct_and_do_not_retain_the_table() {
    let input = [0x41, 0x12, 0x34, 0xaa];
    let view = {
        let table = Opcodes {
            ping: 0x41,
            halt: 0x7f,
            fail: false,
        };
        let (view, suffix) = TableOperation::view(&input)
            .table(&table)
            .with_remainder()
            .unwrap();
        assert_eq!(suffix, &[0xaa]);
        view
    };
    assert_eq!(view.ping().unwrap().value(), 0x1234);

    let plan = {
        let table = Opcodes {
            ping: 0x41,
            halt: 0x7f,
            fail: false,
        };
        TableOperation::Halt.table(&table).prepare().unwrap()
    };
    let mut output = [0_u8; 1];
    assert_eq!(plan.commit_into(&mut output).unwrap().0.as_bytes(), &[0x7f]);

    let table = Opcodes {
        ping: 0x41,
        halt: 0x7f,
        fail: false,
    };
    let mut cursor = TableOperation::cursor(&[0x7f, 0x55]).table(&table);
    assert!(cursor.next().unwrap().unwrap().is_halt());
    assert!(matches!(
        cursor.next(),
        Err(wire_repr::ViewCursorError::Item(
            TableOperationValidationError::Decode(TableOperationDecodeError::UnknownTag {
                tag: 0x55
            })
        ))
    ));
    assert_eq!(cursor.remaining(), &[0x55]);
    let mut unchecked = cursor.unchecked();
    assert!(matches!(
        unchecked.next(),
        Err(wire_repr::ViewCursorError::Item(
            TableOperationDecodeError::UnknownTag { tag: 0x55 }
        ))
    ));
}

#[test]
fn table_named_structs_forward_explicitly_without_retaining_the_table() {
    let input = [0x41, 0x12, 0x34, 0x7f, 0xaa];
    let view = {
        let table = Opcodes {
            ping: 0x41,
            halt: 0x7f,
            fail: false,
        };
        let (view, suffix) = TablePacket::view(&input)
            .table(&table)
            .with_remainder()
            .unwrap();
        assert_eq!(suffix, &[0xaa]);
        view
    };
    assert_eq!(view.first().ping().unwrap().value(), 0x1234);
    assert!(view.second().is_halt());

    let plan = {
        let table = Opcodes {
            ping: 0x41,
            halt: 0x7f,
            fail: false,
        };
        TablePacket {
            first: TableOperation::Ping(Ping { value: 0x1234 }),
            second: TableOperation::Halt,
        }
        .table(&table)
        .prepare()
        .unwrap()
    };
    let mut output = [0_u8; 4];
    assert_eq!(
        plan.commit_into(&mut output).unwrap().0.as_bytes(),
        &[0x41, 0x12, 0x34, 0x7f]
    );

    let table = Opcodes {
        ping: 0x41,
        halt: 0x7f,
        fail: false,
    };
    let cursor_input = [0x7f, 0x7f, 0x55, 0x7f];
    let mut cursor = TablePacket::cursor(&cursor_input).table(&table);
    assert!(cursor.next().unwrap().is_some());
    assert_eq!(cursor.remaining(), &cursor_input[2..]);
    assert!(cursor.next().is_err());
    assert_eq!(cursor.remaining(), &cursor_input[2..]);
    let mut unchecked = cursor.unchecked();
    assert!(unchecked.next().is_err());
    assert_eq!(unchecked.remaining(), &cursor_input[2..]);
}

#[test]
fn borrowed_table_structs_support_tag_only_children() {
    let table = Opcodes {
        ping: 0x41,
        halt: 0x7f,
        fail: false,
    };
    let input = [0x7f, 0xaa, 0xbb];
    let view = BorrowedTablePacket::view(&input)
        .table(&table)
        .without_trailing()
        .unwrap();
    assert!(view.signal().is_halt());
    assert_eq!(view.payload(), &[0xaa, 0xbb]);

    let value = BorrowedTablePacket {
        signal: TableSignal::Halt,
        payload: &[0xaa, 0xbb],
    };
    let mut output = [0_u8; 3];
    let (written, suffix) = value.table(&table).build_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &input);
    assert!(suffix.is_empty());
}

#[test]
fn borrowed_table_structs_support_borrowed_enum_bodies() {
    let table = Opcodes {
        ping: 0x41,
        halt: 0x7f,
        fail: false,
    };
    let input = [0x41, 2, 0xaa, 0xbb];
    let view = BorrowedTableEnvelope::view(&input)
        .table(&table)
        .without_trailing()
        .unwrap();
    let body = view.operation().ping().unwrap();
    assert_eq!(body.payload(), &[0xaa, 0xbb]);

    let value = BorrowedTableEnvelope {
        operation: BorrowedTableOperation::Ping(BorrowedBody {
            length: 2,
            payload: &[0xaa, 0xbb],
        }),
    };
    let mut output = [0_u8; 4];
    let (written, suffix) = value.table(&table).build_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &input);
    assert!(suffix.is_empty());
}

#[test]
fn borrowed_struct_forwarding_and_custom_errors_keep_generated_contracts_coherent() {
    let table = Opcodes {
        ping: 0x41,
        halt: 0x7f,
        fail: false,
    };
    let input = [0x7f, 0xaa, 0xbb];
    let parent = BorrowedTableParent::view(&input)
        .table(&table)
        .without_trailing()
        .unwrap();
    assert!(parent.child().signal().is_halt());
    assert_eq!(parent.child().payload(), &[0xaa, 0xbb]);

    let plan = BorrowedTableParent {
        child: BorrowedTableChild {
            signal: TableSignal::Halt,
            payload: &[0xaa, 0xbb],
        },
    }
    .table(&table)
    .prepare()
    .unwrap();
    let mut output = [0_u8; 3];
    assert_eq!(plan.commit_into(&mut output).unwrap().0.as_bytes(), &input);

    assert!(matches!(
        CustomTableEnvelope::view(&[0x7f, 0xaa])
            .table(&table)
            .without_trailing(),
        Err(CustomTableEnvelopeError::Decode)
    ));
}

#[test]
fn fixed_byte_tags_decode_known_variants_and_preserve_framing() {
    let input = [b'H', b'A', b'L', b'T', 0xaa];
    let (halt, suffix) = ByteOperation::view(&input).with_remainder().unwrap();
    assert!(halt.is_halt());
    assert!(halt.ping().is_none());
    assert_eq!(halt.as_bytes(), b"HALT");
    assert_eq!(suffix, &[0xaa]);
    assert!(core::ptr::eq(suffix.as_ptr(), input[4..].as_ptr()));
    assert!(matches!(
        ByteOperation::view(&input).without_trailing(),
        Err(ByteOperationValidationError::Decode(
            ByteOperationDecodeError::TrailingBytes {
                expected: 4,
                actual: 5,
            }
        ))
    ));

    let ping = ByteOperation::view(b"PING\x12\x34")
        .without_trailing()
        .unwrap();
    assert_eq!(ping.as_bytes(), b"PING\x12\x34");
    assert_eq!(ping.ping().unwrap().value(), 0x1234);

    assert!(matches!(
        ByteOperation::view(b"NOPE").without_trailing(),
        Err(ByteOperationValidationError::Decode(ByteOperationDecodeError::UnknownTag { tag })) if tag == *b"NOPE"
    ));
    assert!(matches!(
        ByteOperation::view(b"PNG").with_remainder(),
        Err(ByteOperationValidationError::Decode(
            ByteOperationDecodeError::InputTooShort {
                required: 4,
                available: 3,
            }
        ))
    ));
}

#[test]
fn fixed_byte_tags_prepare_atomically_and_open_tags_round_trip() {
    let plan = ByteOperation::Ping(Ping { value: 0x1234 })
        .prepare()
        .unwrap();
    assert_eq!(plan.encoded_len(), 6);
    let mut output = [0xa5; 8];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), b"PING\x12\x34");
    assert_eq!(suffix, &mut [0xa5, 0xa5]);

    let initial = [0x5a; 5];
    let mut short = initial;
    assert!(
        ByteOperation::Ping(Ping { value: 0x1234 })
            .build_into(&mut short)
            .is_err()
    );
    assert_eq!(short, initial);

    let raw = [0xff, 0, b'X', 0x80, 0xcc];
    let (unknown, suffix) = OpenByteOperation::view(&raw).with_remainder().unwrap();
    assert_eq!(unknown.as_bytes(), &raw[..4]);
    assert_eq!(unknown.other(), Some(&[0xff, 0, b'X', 0x80]));
    assert!(core::ptr::eq(
        unknown.other().unwrap().as_ptr(),
        raw.as_ptr()
    ));
    assert_eq!(suffix, &[0xcc]);

    let mut output = [0xa5; 5];
    let (written, suffix) = OpenByteOperation::Other([0xff, 0, b'X', 0x80])
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &raw[..4]);
    assert_eq!(suffix, &mut [0xa5]);
}

#[test]
fn open_integer_tags_preserve_the_raw_scalar() {
    let view = OpenIntegerOperation::view(&[0xfe])
        .without_trailing()
        .unwrap();
    assert_eq!(view.other(), Some(0xfe));
    assert_eq!(view.as_bytes(), &[0xfe]);

    let mut output = [0xa5; 2];
    let (written, suffix) = OpenIntegerOperation::Other(0xfe)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[0xfe]);
    assert_eq!(suffix, &mut [0xa5]);
}

#[test]
fn enum_body_validation_is_fail_closed_and_composes_into_parents() {
    let invalid = [1, 0, 0];
    assert!(matches!(
        Operation::view(&invalid).without_trailing(),
        Err(OperationValidationError::Ping(PingError::Zero))
    ));
    assert!(matches!(
        Operation::view(&[1, 0, 0, 0xaa]).without_trailing(),
        Err(OperationValidationError::Ping(PingError::Zero))
    ));
    assert_eq!(
        Operation::view(&invalid)
            .unchecked()
            .without_trailing()
            .unwrap()
            .ping()
            .unwrap()
            .value(),
        0
    );

    let parent = [9, 1, 0, 0, 8, 2, 7];
    assert!(matches!(
        Packet::view(&parent).without_trailing(),
        Err(PacketValidationError::NestedFirst(
            OperationValidationError::Ping(PingError::Zero)
        ))
    ));
    assert_eq!(
        Packet::view(&parent)
            .unchecked()
            .without_trailing()
            .unwrap()
            .first()
            .ping()
            .unwrap()
            .value(),
        0
    );

    let cursor_input = [1, 0, 0, 2];
    let mut cursor = Operation::cursor(&cursor_input);
    assert!(matches!(
        cursor.next(),
        Err(wire_repr::ViewCursorError::Item(
            OperationValidationError::Ping(PingError::Zero)
        ))
    ));
    assert_eq!(cursor.remaining(), &cursor_input);
    let mut unchecked = cursor.unchecked();
    assert_eq!(
        unchecked.next().unwrap().unwrap().ping().unwrap().value(),
        0
    );
    assert_eq!(unchecked.remaining(), &[2]);
}

#[test]
fn table_operation_body_validation_is_fail_closed() {
    let table = Opcodes {
        ping: 0x41,
        halt: 0x7f,
        fail: false,
    };
    let input = [0x41, 0, 0, 0xaa];
    assert!(matches!(
        TableOperation::view(&input)
            .table(&table)
            .without_trailing(),
        Err(TableOperationValidationError::Ping(PingError::Zero))
    ));
    assert_eq!(
        TableOperation::view(&input)
            .table(&table)
            .unchecked()
            .with_remainder()
            .unwrap()
            .0
            .ping()
            .unwrap()
            .value(),
        0
    );
    let mut cursor = TableOperation::cursor(&input).table(&table);
    assert!(matches!(
        cursor.next(),
        Err(wire_repr::ViewCursorError::Item(
            TableOperationValidationError::Ping(PingError::Zero)
        ))
    ));
    assert_eq!(cursor.remaining(), &input);
}

fn cursor_byte_sum(source: &impl ByteSourceCursor) -> u8 {
    source.bytes().fold(0, u8::wrapping_add)
}

/// A parent whose checksum includes the complete prepared enum representation.
#[derive(Wire)]
pub struct EnumCursorChecksum<'wire> {
    /// Sum of the selected enum's physical bytes.
    #[wire(computed = cursor_byte_sum(include(operation)))]
    pub checksum: Computed<u8>,
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
    pub checksum: Computed<u8>,
    /// Tagged operation selected by the checksum.
    pub operation: FragmentedOperation,
    /// Retains the wire lifetime without adding bytes.
    #[wire(rest)]
    pub rest: &'wire [u8],
}

#[test]
fn computed_include_consumes_complete_enum_cursor() {
    let mut output = [0; 4];
    let (written, suffix) = EnumCursorChecksum::builder()
        .operation(Operation::Ping(Ping { value: 1 }))
        .rest(&[])
        .build_into(&mut output)
        .unwrap();

    assert_eq!(written.as_bytes(), &[2, 1, 0, 1]);
    assert_eq!(suffix, &mut []);
    assert_eq!(
        EnumCursorChecksum::view(written.as_bytes())
            .without_trailing()
            .unwrap()
            .checksum(),
        2
    );
}

#[test]
fn fragmented_fixed_tag_cursor_drains_every_tag_segment_before_the_body() {
    let plan = FragmentedOperation::Ping(Ping { value: 1 })
        .prepare()
        .unwrap();
    assert_eq!(plan.bytes().collect::<Vec<_>>(), [2, 0xfe, 0, 1]);

    let unit_plan = FragmentedOperation::Halt.prepare().unwrap();
    assert_eq!(unit_plan.bytes().collect::<Vec<_>>(), [3, 0xfe]);

    let mut output = [0; 5];
    let (written, suffix) = FragmentedEnumCursorChecksum::builder()
        .operation(FragmentedOperation::Ping(Ping { value: 1 }))
        .rest(&[])
        .build_into(&mut output)
        .unwrap();

    assert_eq!(written.as_bytes(), &[1, 2, 0xfe, 0, 1]);
    assert_eq!(suffix, &mut []);
}
