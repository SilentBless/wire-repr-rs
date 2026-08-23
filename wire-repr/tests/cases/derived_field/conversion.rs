use super::*;

#[test]
fn callback_conversion_failure_identifies_the_computed_destination() {
    assert!(matches!(
        OversizedCallback::builder().value(7).prepare(),
        Err(OversizedCallbackEncodeError::ComputedValueNotRepresentable { field: "count" })
    ));
}

#[test]
fn computed_custom_codec_preserves_a_nominal_semantic_type() {
    let payload = [4, 5, 6];
    let mut output = [0; 4];
    let (written, _) = NominalPacket::builder()
        .payload(&payload)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[3, 4, 5, 6]);

    let parsed = NominalPacket::view(written.as_bytes())
        .without_trailing()
        .unwrap();
    assert_eq!(parsed.length(), PayloadLength(3));
    assert_eq!(parsed.payload(), payload);

    let oversized = [0; 256];
    assert!(matches!(
        NominalPacket::builder().payload(&oversized).prepare(),
        Err(NominalPacketEncodeError::ComputedValueNotRepresentable { field: "length" })
    ));
}

#[test]
fn computed_custom_codec_uses_raw_value_type() {
    let payload = [9, 8];
    let mut output = [0; 5];
    let (written, _) = Qualified::builder()
        .payload(&payload)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[0, 0, 2, 9, 8]);
    assert_eq!(
        Qualified::view(written.as_bytes())
            .without_trailing()
            .unwrap()
            .length(),
        2u32
    );
}

#[test]
fn parsing_exposes_encoded_count_without_recomputing() {
    let parsed = RestPacket::view(&[5, 9, 8]).without_trailing().unwrap();
    assert_eq!(parsed.length(), 5);
    assert_eq!(parsed.payload(), [9, 8]);
}
