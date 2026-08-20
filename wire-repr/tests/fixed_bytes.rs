//! Public `Bytes` and `bytes(N)` integration coverage.

use wire_repr::{Bytes, EncodePlan, FixedCodec, wire_repr};

fn render<const N: usize>(plan: impl EncodePlan) -> [u8; N] {
    assert_eq!(plan.encoded_len(), N);
    let mut output = [0; N];
    plan.write_into(&mut output);
    output
}

wire_repr! {
    /// A sequential layout with a borrowed exact-width byte field.
    pub layout SequentialBytes {
        /// The leading arbitrary bytes.
        payload @ 1: bytes(3);
        /// A trailing tag.
        tag @ 2: U8;
    }

    /// An absolute layout with a borrowed exact-width byte field.
    pub absolute layout AbsoluteBytes {
        /// The arbitrary bytes after the leading tag.
        payload @ 1: bytes(2);
        /// The leading tag.
        tag @ 0: U8;
    }
}

#[test]
fn bytes_codec_borrows_exact_width_input_and_plans_that_width() {
    let input = [0xca, 0xfe, 0xff];
    let value = <Bytes<3> as FixedCodec>::decode(&input);
    assert_eq!(value, input);
    assert_eq!(value.as_ptr(), input.as_ptr());
    let plan = <Bytes<3> as FixedCodec>::plan(value).expect("exact-width bytes should plan");
    assert_eq!(render::<3>(plan), input);
    assert_eq!(
        <Bytes<3> as FixedCodec>::plan(&input[..2]),
        Err(wire_repr::ExactWidthError::new(3, 2))
    );
}

#[test]
fn bytes_fields_parse_sequential_and_absolute_layouts_without_semantic_validation() {
    let sequential_input = [0xff, 0x00, 0x80, 0x2a];
    let sequential = SequentialBytes::view(&sequential_input)
        .without_trailing()
        .expect("arbitrary sequential bytes should structurally parse");
    assert_eq!(sequential.payload(), &[0xff, 0x00, 0x80]);
    assert_eq!(sequential.payload().as_ptr(), sequential_input.as_ptr());
    assert_eq!(sequential.tag(), 0x2a);

    let absolute_input = [0x2a, 0xff, 0x00];
    let absolute = AbsoluteBytes::view(&absolute_input)
        .without_trailing()
        .expect("arbitrary absolute bytes should structurally parse");
    assert_eq!(absolute.payload(), &[0xff, 0x00]);
    assert_eq!(absolute.payload().as_ptr(), absolute_input[1..].as_ptr());
    assert_eq!(absolute.tag(), 0x2a);
}
