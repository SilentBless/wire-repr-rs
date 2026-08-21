#![deny(missing_docs, unsafe_code)]
//! Forward field-position coverage.

use wire_repr::{PreparedLayout, Wire};

/// A packet whose payload begins at an explicitly encoded forward position.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct Positioned<'wire> {
    /// Absolute payload position from the representation start.
    pub payload_offset: u8,
    /// Encoded payload length.
    pub payload_length: u8,
    /// Header marker before the positioned payload.
    pub lead: u8,
    /// Borrowed positioned payload.
    #[wire(at = payload_offset, bytes = payload_length)]
    pub payload: &'wire [u8],
    /// Marker after the bounded payload.
    pub tail: u8,
}

/// A fixed field positioned by an earlier encoded source.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct DynamicFixedPosition {
    /// Absolute value position from the representation start.
    pub value_offset: u8,
    /// Value at the encoded absolute position.
    #[wire(at = value_offset, be)]
    pub value: u16,
}

/// A field with a fixed forward position.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct StaticPosition {
    /// Leading marker.
    pub lead: u8,
    /// Value at absolute byte four.
    #[wire(at = 4, be)]
    pub value: u16,
    /// Trailing marker.
    pub tail: u8,
}

#[test]
fn decoding_uses_the_explicit_forward_position() {
    let input = [5, 2, 9, 0xaa, 0xbb, 1, 2, 3, 0xcc];
    let (parsed, suffix) = Positioned::view(&input).with_remainder().unwrap();
    assert_eq!(parsed.as_bytes(), &input[..8]);
    assert_eq!(parsed.payload(), &input[5..7]);
    assert_eq!(parsed.tail(), 3);
    assert_eq!(suffix, &[0xcc]);

    let error = Positioned::view(&[2, 1, 9, 7])
        .with_remainder()
        .unwrap_err();
    assert!(matches!(
        error,
        PositionedDecodeError::PositionBeforeCursor {
            field: "payload",
            position: 2,
            cursor: 3,
        }
    ));
}

#[test]
fn preparation_retains_requested_position_and_canonicalizes_length() {
    let payload = [4, 5];
    let plan = Positioned {
        payload_offset: 6,
        payload_length: 99,
        lead: 8,
        payload: &payload,
        tail: 7,
    }
    .prepare()
    .unwrap();
    assert_eq!(plan.encoded_len(), 9);

    let mut output = [0xa5; 10];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[6, 2, 8, 0, 0, 0, 4, 5, 7]);
    assert_eq!(suffix, &mut [0xa5]);

    let parsed = Positioned::view(written.as_bytes())
        .without_trailing()
        .unwrap();
    assert_eq!(parsed.payload_offset(), 6);
    assert_eq!(parsed.payload_length(), 2);
}

#[test]
fn backward_positions_and_short_outputs_are_atomic_failures() {
    let payload = [4];
    let error = match (Positioned {
        payload_offset: 2,
        payload_length: 1,
        lead: 8,
        payload: &payload,
        tail: 7,
    }
    .prepare())
    {
        Err(error) => error,
        Ok(_) => panic!("a backward field position should fail during preparation"),
    };
    assert!(matches!(
        error,
        PositionedEncodeError::PositionBeforeCursor {
            field: "payload",
            position: 2,
            cursor: 3,
        }
    ));

    let plan = Positioned {
        payload_offset: 5,
        payload_length: 1,
        lead: 8,
        payload: &payload,
        tail: 7,
    }
    .prepare()
    .unwrap();
    let mut short = [0xa5; 6];
    assert!(plan.commit_into(&mut short).is_err());
    assert_eq!(short, [0xa5; 6]);
}

#[test]
fn static_positions_encode_forward_gaps() {
    let input = [1, 0xaa, 0xbb, 0xcc, 0x12, 0x34, 2, 0xdd];
    let (parsed, suffix) = StaticPosition::view(&input).with_remainder().unwrap();
    assert_eq!(parsed.value(), 0x1234);
    assert_eq!(parsed.as_bytes(), &input[..7]);
    assert_eq!(suffix, &[0xdd]);

    let plan = StaticPosition {
        lead: 1,
        value: 0x1234,
        tail: 2,
    }
    .prepare()
    .unwrap();
    let mut output = [0xa5; 8];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[1, 0, 0, 0, 0x12, 0x34, 2]);
    assert_eq!(suffix, &mut [0xa5]);
}

#[test]
fn dynamic_fixed_positions_retain_validated_field_spans() {
    let input = [4, 0xaa, 0xbb, 0xcc, 0x12, 0x34, 0xdd];
    let (view, suffix) = DynamicFixedPosition::view(&input).with_remainder().unwrap();
    assert_eq!(view.value_offset(), 4);
    assert_eq!(view.value(), 0x1234);
    assert_eq!(view.as_bytes(), &input[..6]);
    assert_eq!(suffix, &[0xdd]);

    let copied = view;
    assert_eq!(copied.value(), 0x1234);
}
