#![deny(missing_docs, unsafe_code)]
//! Static tagged enum derive coverage.

use wire_repr::{PreparedLayout, Wire};

/// A fixed body carried by one operation variant.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct Ping {
    /// Network-order ping value.
    #[wire(be)]
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
    opcode_error = OpcodeMapError,
    unknown = reject
)]
pub enum MappedOperation {
    /// Carries a fixed ping body.
    #[wire(opcode = Opcode::Ping)]
    Ping(Ping),
    /// Carries no body.
    #[wire(opcode = Opcode::Halt)]
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
        Err(OperationDecodeError::UnknownTag { tag: 3 })
    ));
    let error = Operation::view(&[]).without_trailing().unwrap_err();
    assert!(matches!(
        error,
        OperationDecodeError::InputTooShort {
            required: 1,
            available: 0,
        }
    ));
    assert_eq!(
        error.to_string(),
        "tag needs 1 byte, but only 0 bytes remain"
    );

    let error = Operation::view(&[3]).without_trailing().unwrap_err();
    assert!(matches!(error, OperationDecodeError::UnknownTag { tag: 3 }));
    assert_eq!(error.to_string(), "unknown wire tag 3");

    let error = Operation::view(&[1, 0]).without_trailing().unwrap_err();
    assert!(matches!(
        error,
        OperationDecodeError::Ping(PingDecodeError::InputTooShort {
            field: "value",
            required: 2,
            available: 1,
        })
    ));
    assert_eq!(
        error.to_string(),
        "wire decode failed in variant `Ping`: field `value` needs 2 bytes, but only 1 byte remains"
    );

    let error = Operation::view(&[2, 99]).without_trailing().unwrap_err();
    assert!(matches!(
        error,
        OperationDecodeError::TrailingBytes {
            expected: 1,
            actual: 2,
        }
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
        Err(PacketDecodeError::First(OperationDecodeError::UnknownTag {
            tag: 3,
        }))
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
        Err(MappedOperationDecodeError::UnknownTag { tag: 0x55 })
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
        Err(MappedOperationDecodeError::OpcodeMapping(OpcodeMapError))
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
        Err(ByteOperationDecodeError::TrailingBytes {
            expected: 4,
            actual: 5,
        })
    ));

    let ping = ByteOperation::view(b"PING\x12\x34")
        .without_trailing()
        .unwrap();
    assert_eq!(ping.as_bytes(), b"PING\x12\x34");
    assert_eq!(ping.ping().unwrap().value(), 0x1234);

    assert!(matches!(
        ByteOperation::view(b"NOPE").without_trailing(),
        Err(ByteOperationDecodeError::UnknownTag { tag }) if tag == *b"NOPE"
    ));
    assert!(matches!(
        ByteOperation::view(b"PNG").with_remainder(),
        Err(ByteOperationDecodeError::InputTooShort {
            required: 4,
            available: 3,
        })
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
