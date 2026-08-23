use super::*;

#[test]
fn enum_byte_source_emits_tag_before_selected_body() {
    let plan = Operation::Ping(Ping { value: 0x1234 }).prepare().unwrap();
    let mut sink = RecordingSink::default();
    plan.emit_to(&mut sink);
    assert_eq!(sink.writes, [vec![1], vec![0x12, 0x34]]);
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
