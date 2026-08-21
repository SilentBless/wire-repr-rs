#![deny(missing_docs, unsafe_code)]

//! Tagged-choice generated API coverage.

use wire_repr::{Discriminant, wire_repr};

wire_repr! {
    /// A statically tagged message with a forward body declaration.
    pub choice WideMessage {
        tagged by kind: BeU24;
        Ping = 0x01_0203: WideBody;
        Halt = 0x0a_0b0c;
    }
    /// A body selected by a static or dynamic message tag.
    pub layout PingBody { value: BeU16; }
    /// A forward-declared body used to prove non-byte tag handling.
    pub layout WideBody { value: BeU16; }
    /// A statically tagged byte-sized message.
    pub choice Message {
        tagged by kind: U8;
        Ping = 1: PingBody;
        Halt = 2;
    }
    /// A runtime-selected message.
    pub choice DynamicMessage {
        context table: TagTable;
        tagged by kind: U8 using table;
        Ping: PingBody;
        Halt;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TagError {
    BadVersion,
    EncodeDenied,
}

struct TagTable {
    version: u8,
}

impl Discriminant<u8, DynamicMessageCase> for TagTable {
    type Error = TagError;

    fn resolve(&self, raw: u8) -> Result<Option<DynamicMessageCase>, Self::Error> {
        match (self.version, raw) {
            (1, 1) | (2, 16 | 17) => Ok(Some(DynamicMessageCase::Ping)),
            (1, 2) | (2, 18) => Ok(Some(DynamicMessageCase::Halt)),
            (0, _) => Err(TagError::BadVersion),
            _ => Ok(None),
        }
    }

    fn encode(&self, case: DynamicMessageCase) -> Result<u8, Self::Error> {
        match (self.version, case) {
            (1, DynamicMessageCase::Ping) => Ok(1),
            (1, DynamicMessageCase::Halt) => Ok(2),
            (2, DynamicMessageCase::Ping) => Ok(17),
            (2, DynamicMessageCase::Halt) => Ok(18),
            (3, _) => Err(TagError::EncodeDenied),
            _ => Err(TagError::BadVersion),
        }
    }
}

#[test]
fn generated_choice_write_errors_implement_error() {
    fn assert_error<T: core::error::Error>() {}

    assert_error::<MessageWriteError>();
    assert_error::<DynamicMessageWriteError>();
}

#[test]
fn static_choice_parses_forward_non_byte_tag_and_bodyless_cases() {
    let (parsed, suffix) = WideMessage::view(&[1, 2, 3, 0xab, 0xcd, 9])
        .with_remainder()
        .expect("known wide choice parses");
    assert_eq!(parsed.kind(), 0x01_0203);
    assert_eq!(parsed.case(), WideMessageCase::Ping);
    assert_eq!(parsed.as_bytes(), &[1, 2, 3, 0xab, 0xcd]);
    assert_eq!(suffix, &[9]);
    assert!(matches!(parsed.variant(), WideMessageVariant::Ping(body) if body.value() == 0xabcd));

    let parsed = Message::view(&[2])
        .without_trailing()
        .expect("unit choice parses");
    assert_eq!(parsed.case(), MessageCase::Halt);
    assert_eq!(parsed.as_bytes(), &[2]);
    assert!(matches!(parsed.variant(), MessageVariant::Halt));
    assert!(matches!(
        Message::view(&[1, 0x12]).with_remainder(),
        Err(MessageError::Ping(_))
    ));
    assert!(matches!(
        WideMessage::view(&[1, 2]).without_trailing(),
        Err(WideMessageError::TagTooShort {
            expected: 3,
            actual: 2
        })
    ));
    assert!(matches!(
        Message::view(&[2, 9]).without_trailing(),
        Err(MessageError::TrailingBytes {
            expected: 1,
            actual: 2
        })
    ));
}

#[test]
fn unknown_policy_retains_exact_bytes_and_rebuilds_without_a_resolver() {
    assert!(matches!(
        Message::view(&[7]).without_trailing(),
        Err(MessageError::UnknownTag { raw: 7 })
    ));
    assert!(matches!(
        Message::view(&[7, 8])
            .accept_unknown_exact(2)
            .with_remainder(),
        Err(MessageError::UnknownBodyTooShort {
            expected: 2,
            actual: 1
        })
    ));

    let (view, suffix) = Message::view(&[7, 8, 9])
        .accept_unknown_exact(1)
        .with_remainder()
        .expect("exact unknown body parses");
    assert_eq!(view.kind(), 7);
    assert_eq!(view.as_bytes(), &[7, 8]);
    assert_eq!(suffix, &[9]);
    let MessageVariant::Unknown(retained) = view.variant() else {
        panic!("expected retained unknown variant");
    };
    assert_eq!(retained.kind(), 7);
    assert_eq!(retained.as_bytes(), &[8]);

    let (remainder, suffix) = Message::view(&[7, 8, 9])
        .accept_unknown_remainder()
        .with_remainder()
        .expect("remainder unknown body parses");
    assert_eq!(remainder.as_bytes(), &[7, 8, 9]);
    assert!(suffix.is_empty());

    let static_unknown = Message::view(&[7, 8, 9])
        .accept_unknown_exact(2)
        .without_trailing()
        .expect("exact retained unknown rebuild source parses");
    let MessageVariant::Unknown(static_unknown) = static_unknown.variant() else {
        panic!("expected retained unknown variant");
    };
    let mut output = [0xee; 5];
    let (built, suffix) = MessageBuilder::unknown(*static_unknown)
        .build_into(&mut output)
        .expect("static retained unknown rebuilds");
    assert_eq!(built.as_bytes(), &[7, 8, 9]);
    assert_eq!(built.kind(), 7);
    assert_eq!(built.case(), MessageCase::Unknown);
    assert_eq!(suffix, &[0xee, 0xee]);

    let dynamic_unknown = DynamicMessage::view(&[99, 4, 5])
        .table(&TagTable { version: 3 })
        .accept_unknown_remainder()
        .without_trailing()
        .expect("remainder retained unknown rebuild source parses");
    let DynamicMessageVariant::Unknown(dynamic_unknown) = dynamic_unknown.variant() else {
        panic!("expected retained unknown variant");
    };
    let (built, suffix) = DynamicMessageBuilder::unknown(*dynamic_unknown)
        .build_into(&mut output)
        .expect("dynamic retained unknown rebuilds without a resolver");
    assert_eq!(built.as_bytes(), &[99, 4, 5]);
    assert_eq!(built.kind(), 99);
    assert_eq!(built.case(), DynamicMessageCase::Unknown);
    assert_eq!(suffix, &[0xee, 0xee]);
}

#[test]
fn prepared_static_body_commit_is_atomic_and_preserves_the_suffix() {
    let body = PingBodyBuilder::new()
        .value(0xabcd)
        .prepare()
        .expect("caller prepares the body plan");
    let plan = MessageBuilder::ping(body)
        .prepare()
        .expect("choice composes the prepared body plan");
    assert_eq!(plan.encoded_len(), 3);

    let mut output = [0xee; 5];
    let before = output;
    assert!(plan.commit_into(&mut output[..2]).is_err());
    assert_eq!(output, before);

    let (built, suffix) = MessageBuilder::ping(
        PingBodyBuilder::new()
            .value(0xabcd)
            .prepare()
            .expect("caller prepares a body plan"),
    )
    .build_into(&mut output)
    .expect("exact-sized body plan commits");
    assert_eq!(built.as_bytes(), &[1, 0xab, 0xcd]);
    assert_eq!(built.case(), MessageCase::Ping);
    assert_eq!(suffix, &[0xee, 0xee]);
}

#[test]
fn dynamic_choice_distinguishes_missing_unknown_and_resolver_errors() {
    let version_one = TagTable { version: 1 };
    let version_two = TagTable { version: 2 };

    let parsed = DynamicMessage::view(&[1, 0x12, 0x34])
        .table(&version_one)
        .without_trailing()
        .expect("version one resolves its ping tag");
    assert_eq!(parsed.case(), DynamicMessageCase::Ping);
    assert_eq!(parsed.as_bytes(), &[1, 0x12, 0x34]);
    let parsed = DynamicMessage::view(&[17, 0x12, 0x34])
        .table(&version_two)
        .without_trailing()
        .expect("version two resolves its ping tag");
    assert_eq!(parsed.case(), DynamicMessageCase::Ping);

    let alias = DynamicMessage::view(&[16, 0x12, 0x34])
        .table(&version_two)
        .without_trailing()
        .expect("version two accepts the ping alias");
    assert_eq!(alias.kind(), 16);
    assert_eq!(alias.case(), DynamicMessageCase::Ping);
    let mut output = [0xee; 3];
    let (built, _) = DynamicMessageBuilder::ping(
        PingBodyBuilder::new()
            .value(0x1234)
            .prepare()
            .expect("caller prepares an aliased body plan"),
    )
    .table(&version_two)
    .build_into(&mut output)
    .expect("dynamic encoding uses the canonical ping tag");
    assert_eq!(built.as_bytes(), &[17, 0x12, 0x34]);

    assert!(matches!(
        DynamicMessage::view(&[1]).without_trailing(),
        Err(DynamicMessageError::MissingContext)
    ));
    assert!(matches!(
        DynamicMessage::view(&[99, 8])
            .table(&version_one)
            .without_trailing(),
        Err(DynamicMessageError::UnknownTag { raw: 99 })
    ));
    assert!(matches!(
        DynamicMessage::view(&[99, 8])
            .table(&version_one)
            .accept_unknown_exact(1)
            .without_trailing(),
        Ok(view) if matches!(view.variant(), DynamicMessageVariant::Unknown(unknown) if unknown.as_bytes() == [8])
    ));
    assert!(matches!(
        DynamicMessage::view(&[1])
            .table(&TagTable { version: 0 })
            .without_trailing(),
        Err(DynamicMessageError::Resolver(TagError::BadVersion))
    ));

    assert!(matches!(
        DynamicMessageBuilder::halt().prepare(),
        Err(DynamicMessageWriteError::MissingContext)
    ));
    let mut output = [0xee; 3];
    let before = output;
    assert!(matches!(
        DynamicMessageBuilder::halt()
            .table(&TagTable { version: 3 })
            .build_into(&mut output),
        Err(DynamicMessageWriteError::Resolver(TagError::EncodeDenied))
    ));
    assert_eq!(output, before);

    let (built, suffix) = DynamicMessageBuilder::halt()
        .table(&version_two)
        .build_into(&mut output)
        .expect("version two selects its encoded tag");
    assert_eq!(built.as_bytes(), &[18]);
    assert_eq!(built.kind(), 18);
    assert_eq!(built.case(), DynamicMessageCase::Halt);
    assert_eq!(suffix, &[0xee, 0xee]);
}
