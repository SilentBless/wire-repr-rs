//! Wide fixed-header output prepares semantic fields before atomic capacity preflight.
//! mode: bytes
//! pair: wide_fixed_append = owned_wide_generated_append / owned_wide_handwritten_append
//! tolerance: 10%
//! weights: instructions=1, branches=4, calls=8, panic_paths=16

#![allow(dead_code)]

use bytes::BytesMut;
use core::hint::black_box;
use wire_repr::{PreparedLayout, Wire};

#[derive(Wire)]
struct WideFixed {
    #[wire(be)]
    word_00: u32,
    #[wire(be)]
    word_01: u32,
    #[wire(be)]
    word_02: u32,
    #[wire(be)]
    word_03: u32,
    #[wire(be)]
    word_04: u32,
    #[wire(be)]
    word_05: u32,
    #[wire(be)]
    word_06: u32,
    #[wire(be)]
    word_07: u32,
    #[wire(be)]
    word_08: u32,
    #[wire(be)]
    word_09: u32,
    #[wire(be)]
    word_10: u32,
    #[wire(be)]
    word_11: u32,
    #[wire(be)]
    word_12: u32,
    #[wire(be)]
    word_13: u32,
    #[wire(be)]
    word_14: u32,
    #[wire(be)]
    word_15: u32,
    #[wire(be)]
    word_16: u32,
    #[wire(be)]
    word_17: u32,
    #[wire(be)]
    word_18: u32,
    #[wire(be)]
    word_19: u32,
    #[wire(be)]
    word_20: u32,
    #[wire(be)]
    word_21: u32,
    #[wire(be)]
    word_22: u32,
    #[wire(be)]
    word_23: u32,
    #[wire(be)]
    word_24: u32,
    #[wire(be)]
    word_25: u32,
    #[wire(be)]
    word_26: u32,
    #[wire(be)]
    word_27: u32,
    #[wire(be)]
    word_28: u32,
    #[wire(be)]
    word_29: u32,
    #[wire(be)]
    word_30: u32,
    #[wire(be)]
    word_31: u32,
}

#[inline(never)]
pub fn owned_wide_generated_append(input: &[u8; 128], output: &mut BytesMut) -> usize {
    let header = WideFixed {
        word_00: u32::from_be_bytes([input[0], input[1], input[2], input[3]]),
        word_01: u32::from_be_bytes([input[4], input[5], input[6], input[7]]),
        word_02: u32::from_be_bytes([input[8], input[9], input[10], input[11]]),
        word_03: u32::from_be_bytes([input[12], input[13], input[14], input[15]]),
        word_04: u32::from_be_bytes([input[16], input[17], input[18], input[19]]),
        word_05: u32::from_be_bytes([input[20], input[21], input[22], input[23]]),
        word_06: u32::from_be_bytes([input[24], input[25], input[26], input[27]]),
        word_07: u32::from_be_bytes([input[28], input[29], input[30], input[31]]),
        word_08: u32::from_be_bytes([input[32], input[33], input[34], input[35]]),
        word_09: u32::from_be_bytes([input[36], input[37], input[38], input[39]]),
        word_10: u32::from_be_bytes([input[40], input[41], input[42], input[43]]),
        word_11: u32::from_be_bytes([input[44], input[45], input[46], input[47]]),
        word_12: u32::from_be_bytes([input[48], input[49], input[50], input[51]]),
        word_13: u32::from_be_bytes([input[52], input[53], input[54], input[55]]),
        word_14: u32::from_be_bytes([input[56], input[57], input[58], input[59]]),
        word_15: u32::from_be_bytes([input[60], input[61], input[62], input[63]]),
        word_16: u32::from_be_bytes([input[64], input[65], input[66], input[67]]),
        word_17: u32::from_be_bytes([input[68], input[69], input[70], input[71]]),
        word_18: u32::from_be_bytes([input[72], input[73], input[74], input[75]]),
        word_19: u32::from_be_bytes([input[76], input[77], input[78], input[79]]),
        word_20: u32::from_be_bytes([input[80], input[81], input[82], input[83]]),
        word_21: u32::from_be_bytes([input[84], input[85], input[86], input[87]]),
        word_22: u32::from_be_bytes([input[88], input[89], input[90], input[91]]),
        word_23: u32::from_be_bytes([input[92], input[93], input[94], input[95]]),
        word_24: u32::from_be_bytes([input[96], input[97], input[98], input[99]]),
        word_25: u32::from_be_bytes([input[100], input[101], input[102], input[103]]),
        word_26: u32::from_be_bytes([input[104], input[105], input[106], input[107]]),
        word_27: u32::from_be_bytes([input[108], input[109], input[110], input[111]]),
        word_28: u32::from_be_bytes([input[112], input[113], input[114], input[115]]),
        word_29: u32::from_be_bytes([input[116], input[117], input[118], input[119]]),
        word_30: u32::from_be_bytes([input[120], input[121], input[122], input[123]]),
        word_31: u32::from_be_bytes([input[124], input[125], input[126], input[127]]),
    };
    let Ok(plan) = header.prepare() else {
        return 0;
    };
    plan.commit_into(output)
        .map_or(0, |written| written.as_bytes().len())
}

#[inline(never)]
pub fn owned_wide_handwritten_append(input: &[u8; 128], output: &mut BytesMut) -> usize {
    const REQUIRED: usize = 128;
    if output.capacity() - output.len() < REQUIRED {
        return 0;
    }
    output.extend_from_slice(input);
    REQUIRED
}

#[test]
fn owned_wide_fixed_append_pair_preserves_atomic_no_growth_contract() {
    let input = core::array::from_fn(|index| index as u8);

    for (prefix, capacity, expected_success) in [
        (&[][..], 128, true),
        (&[0xaa, 0xbb][..], 130, true),
        (&[0xaa, 0xbb, 0xcc][..], 130, false),
    ] {
        let mut generated = BytesMut::with_capacity(capacity);
        let mut handwritten = BytesMut::with_capacity(capacity);
        generated.extend_from_slice(prefix);
        handwritten.extend_from_slice(prefix);

        let generated_pointer = generated.as_ptr();
        let handwritten_pointer = handwritten.as_ptr();
        let generated_capacity = generated.capacity();
        let handwritten_capacity = handwritten.capacity();
        let generated_before = generated.clone();
        let handwritten_before = handwritten.clone();

        let generated_written = owned_wide_generated_append(&input, black_box(&mut generated));
        let handwritten_written =
            owned_wide_handwritten_append(&input, black_box(&mut handwritten));

        assert_eq!(generated_written, handwritten_written);
        assert_eq!(generated_written, if expected_success { 128 } else { 0 });
        assert_eq!(generated.as_ptr(), generated_pointer);
        assert_eq!(handwritten.as_ptr(), handwritten_pointer);
        assert_eq!(generated.capacity(), generated_capacity);
        assert_eq!(handwritten.capacity(), handwritten_capacity);
        assert_eq!(generated, handwritten);

        if expected_success {
            assert_eq!(generated.len(), prefix.len() + 128);
            assert_eq!(&generated[..prefix.len()], prefix);
            assert_eq!(&generated[prefix.len()..], input);
        } else {
            assert_eq!(generated, generated_before);
            assert_eq!(handwritten, handwritten_before);
        }
    }
}
