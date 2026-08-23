use super::*;

#[test]
fn computed_derivation_and_exact_source_validation_share_the_byte_source_abi() {
    let payload = [1, 2, 3];
    let mut output = [0; 4];
    let (written, _) = ValidatedChecksum::builder()
        .payload(&payload)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[6, 1, 2, 3]);
    assert!(
        ValidatedChecksum::view(written.as_bytes())
            .without_trailing()
            .is_ok()
    );

    assert!(matches!(
        ValidatedChecksum::view(&[7, 1, 2, 3]).without_trailing(),
        Err(ChecksumError::Mismatch)
    ));
    let unchecked = ValidatedChecksum::view(&[7, 1, 2, 3])
        .unchecked()
        .without_trailing()
        .unwrap();
    assert_eq!(unchecked.checksum(), 7);
}

#[test]
fn table_bound_borrowed_builder_combines_computation_with_canonical_framing() {
    let payload = [7, 8];
    let mut short = [0xa5; 5];
    {
        let table = CallbackTable { tag: 0x31 };
        assert!(
            CallbackPayload::builder()
                .payload(&payload)
                .selected(CallbackOperation::Ping(CallbackBody { value: 4 }))
                .table(&table)
                .build_into(&mut short)
                .is_err()
        );
    }
    assert_eq!(short, [0xa5; 5]);

    let plan = {
        let table = CallbackTable { tag: 0x31 };
        CallbackPayload::builder()
            .payload(&payload)
            .selected(CallbackOperation::Ping(CallbackBody { value: 4 }))
            .table(&table)
            .prepare()
            .unwrap()
    };
    assert_eq!(plan.encoded_len(), 6);

    let mut output = [0; 6];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[0x35, 2, 7, 8, 0x31, 4]);
    assert_eq!(suffix, &mut []);

    let table = CallbackTable { tag: 0x31 };
    let view = CallbackPayload::view(written.as_bytes())
        .table(&table)
        .without_trailing()
        .unwrap();
    assert_eq!(view.checksum(), 0x35);
    assert_eq!(view.length(), 2);
    assert_eq!(view.payload(), payload);
    assert_eq!(view.selected().ping().unwrap().value(), 4);
}
