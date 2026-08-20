use wire_repr::{BeU16, FixedCodec, U24RangeError, wire_repr};

type ExternalCodec = BeU16;

wire_repr! {
    pub layout Header {
        field hardware_type: HardwareType;
        field payload_size: PayloadSize;
    }

    /// A hardware address family.
    pub scalar HardwareType: BeU16;
    pub scalar PayloadSize: BeU24;

    pub layout ExternalHeader {
        field code: crate::ExternalCodec;
    }
}

#[test]
fn scalars_are_nominal_fixed_codecs_and_layout_fields_use_direct_paths() {
    let hardware = HardwareType::new(1);
    let _: HardwareType = 1u16.into();
    let raw: u16 = hardware.into();
    assert_eq!(raw, 1);
    assert_eq!(hardware.raw(), 1);
    assert_eq!(HardwareType::WIDTH, BeU16::WIDTH);
    assert_eq!(HardwareType::decode(&[0, 1]), hardware);

    let input = [0, 1, 0x12, 0x34, 0x56, 0xaa];
    let (view, suffix) = Header::view(&input).with_remainder().unwrap();
    assert_eq!(suffix, &[0xaa]);
    assert_eq!(view.hardware_type(), hardware);
    assert_eq!(view.payload_size(), PayloadSize::new(0x12_3456));
    assert_eq!(view.as_bytes(), &input[..5]);
    assert!(Header::view(&input).without_trailing().is_err());
    assert_eq!(
        Header::view(&input[..5])
            .without_trailing()
            .unwrap()
            .as_bytes(),
        &input[..5]
    );

    assert_eq!(
        ExternalHeader::view(&[0x12, 0x34])
            .without_trailing()
            .unwrap()
            .code(),
        0x1234
    );
}

#[test]
fn scalar_fields_mutate_and_builder_delegates_u24_planning() {
    let mut bytes = [0, 1, 0, 0, 2];
    let mut view = HeaderViewMut::parse_exact_mut(&mut bytes).unwrap();
    view.set_hardware_type(HardwareType::new(7)).unwrap();
    view.set_payload_size(PayloadSize::new(0x0000_abcd))
        .unwrap();
    assert_eq!(view.as_bytes(), &[0, 7, 0, 0xab, 0xcd]);

    let mut output = [0xa5; 6];
    let (built, suffix) = HeaderBuilder::new()
        .hardware_type(HardwareType::new(1))
        .payload_size(PayloadSize::new(0x00ff_ffff))
        .build_into(&mut output)
        .unwrap();
    assert_eq!(built.as_bytes(), &[0, 1, 0xff, 0xff, 0xff]);
    assert_eq!(suffix, &[0xa5]);

    let mut unchanged = [0xa5; 5];
    assert!(matches!(
        HeaderBuilder::new().hardware_type(HardwareType::new(1)).payload_size(PayloadSize::new(0x0100_0000)).build_into(&mut unchanged),
        Err(HeaderWriteError::FieldPayloadSize(error)) if error == U24RangeError::new(0x0100_0000)
    ));
    assert_eq!(unchanged, [0xa5; 5]);
}
