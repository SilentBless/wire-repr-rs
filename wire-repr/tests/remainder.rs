use wire_repr::wire_repr;

wire_repr! {
    pub layout EthernetEnvelope {
        field destination: bytes(6);
        field source: bytes(6);
        field ether_type: BeU16;
        field payload: remainder;
    }

    pub layout PositionedRemainder {
        field payload: remainder { position: 4; }
        align { position: 3; boundary: 4; }
        padding { position: 2; length: 2; }
        field head: U8 { position: 1; }
    }
}

const DESTINATION: &[u8; 6] = &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
const SOURCE: &[u8; 6] = &[0x00, 0x11, 0x22, 0x33, 0x44, 0x55];

#[test]
fn implicit_terminal_remainder_borrows_every_byte_after_the_header() {
    let input = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x08, 0x00, 0x45,
        0x00, 0x12,
    ];
    let (view, suffix) = EthernetEnvelopeView::parse_prefix(&input).expect("valid envelope");

    assert_eq!(view.as_bytes(), &input);
    assert_eq!(view.as_bytes().as_ptr(), input.as_ptr());
    assert_eq!(view.destination(), DESTINATION);
    assert_eq!(view.source(), SOURCE);
    assert_eq!(view.ether_type(), 0x0800);
    assert_eq!(view.payload(), &[0x45, 0x00, 0x12]);
    assert_eq!(view.payload().as_ptr(), input[14..].as_ptr());
    assert!(suffix.is_empty());

    let exact = EthernetEnvelopeView::parse_exact(&input).expect("same complete input");
    assert_eq!(exact.as_bytes(), view.as_bytes());
    assert_eq!(exact.payload(), view.payload());
}

#[test]
fn terminal_remainder_may_be_empty() {
    let input = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x08, 0x00,
    ];
    let view = EthernetEnvelopeView::parse_exact(&input).expect("empty payload is valid");

    assert_eq!(view.as_bytes(), &input);
    assert!(view.payload().is_empty());
}

#[test]
fn builder_distinguishes_an_empty_remainder_from_a_missing_field() {
    let mut output = [0xa5; 16];
    let (view, suffix) = EthernetEnvelopeBuilder::new()
        .destination(DESTINATION)
        .source(SOURCE)
        .ether_type(0x0800)
        .payload(&[])
        .build_into(&mut output)
        .expect("an explicitly empty payload is present");

    assert_eq!(view.as_bytes().len(), 14);
    assert!(view.payload().is_empty());
    assert_eq!(suffix, &[0xa5, 0xa5]);
}

#[test]
fn mutable_remainder_accessor_is_confined_to_the_remainder() {
    let mut input = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x08, 0x00, 0x45,
        0x00, 0x12,
    ];
    {
        let mut view =
            EthernetEnvelopeViewMut::parse_exact_mut(&mut input).expect("valid envelope");
        view.payload_mut().copy_from_slice(&[0x60, 0x00, 0x34]);
        assert_eq!(view.destination(), DESTINATION);
        assert_eq!(view.source(), SOURCE);
        assert_eq!(view.ether_type(), 0x0800);
        assert_eq!(view.payload(), &[0x60, 0x00, 0x34]);
    }
    assert_eq!(
        input,
        [
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x08, 0x00,
            0x60, 0x00, 0x34,
        ]
    );
}

#[test]
fn remainder_builder_copies_atomically_and_preserves_the_output_suffix() {
    let mut output = [0xa5; 20];
    let (view, suffix) = EthernetEnvelopeBuilder::new()
        .destination(DESTINATION)
        .source(SOURCE)
        .ether_type(0x0800)
        .payload(&[0x45, 0x00, 0x12])
        .build_into(&mut output)
        .expect("complete envelope");

    assert_eq!(
        view.as_bytes(),
        &[
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x08, 0x00,
            0x45, 0x00, 0x12,
        ]
    );
    assert_eq!(view.payload(), &[0x45, 0x00, 0x12]);
    assert_eq!(suffix, &[0xa5, 0xa5, 0xa5]);
    assert_eq!(output[17..], [0xa5, 0xa5, 0xa5]);
}

#[test]
fn remainder_builder_failures_leave_output_unchanged() {
    let initial = [0x5a; 16];
    let mut output = initial;
    assert!(matches!(
        EthernetEnvelopeBuilder::new()
            .destination(DESTINATION)
            .source(SOURCE)
            .ether_type(0x0800)
            .build_into(&mut output),
        Err(EthernetEnvelopeWriteError::MissingField { field: "payload" })
    ));
    assert_eq!(output, initial);

    let mut short = [0x3c; 15];
    let before = short;
    assert!(matches!(
        EthernetEnvelopeBuilder::new()
            .destination(DESTINATION)
            .source(SOURCE)
            .ether_type(0x0800)
            .payload(&[0x45, 0x00])
            .build_into(&mut short),
        Err(EthernetEnvelopeWriteError::OutputTooShort {
            expected: 16,
            actual: 15,
        })
    ));
    assert_eq!(short, before);
}

#[test]
fn physical_reordering_padding_and_alignment_preserve_the_remainder_boundary() {
    let mut input = [0x11, 0xa1, 0xa2, 0xa3, 0x20, 0x21];
    {
        let mut view =
            PositionedRemainderViewMut::parse_exact_mut(&mut input).expect("valid layout");
        assert_eq!(view.head(), 0x11);
        assert_eq!(view.payload(), &[0x20, 0x21]);
        assert_eq!(view.payload().as_ptr(), view.as_bytes()[4..].as_ptr());
        view.payload_mut().copy_from_slice(&[0x30, 0x31]);
    }
    assert_eq!(input, [0x11, 0xa1, 0xa2, 0xa3, 0x30, 0x31]);

    let mut output = [0xa5; 8];
    let (view, suffix) = PositionedRemainderBuilder::new()
        .payload(&[0x40, 0x41])
        .head(0x12)
        .build_into(&mut output)
        .expect("complete layout");

    assert_eq!(view.as_bytes(), &[0x12, 0xa5, 0xa5, 0xa5, 0x40, 0x41]);
    assert_eq!(view.payload(), &[0x40, 0x41]);
    assert_eq!(suffix, &[0xa5, 0xa5]);
}
