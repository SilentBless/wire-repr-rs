//! Owned public decoding includes shared-owner drops and structured error paths.
//! mode: bytes
//! pair: fixed = owned_generated_fixed_decode / owned_handwritten_fixed_decode
//! tolerance: 550%
//! weights: instructions=1, branches=4, calls=8, panic_paths=16

#![allow(dead_code)]

use bytes::Bytes;
use core::hint::black_box;
use wire_repr::Wire;

#[derive(Wire)]
struct Fixed {
    lead: u8,
    #[wire(be)]
    word: u16,
}

#[inline(never)]
pub fn owned_generated_fixed_decode(input: Bytes) -> u16 {
    Fixed::view(input)
        .without_trailing()
        .map_or(u16::MAX, |frame| frame.word() ^ u16::from(frame.lead()))
}

#[inline(never)]
pub fn owned_handwritten_fixed_decode(input: Bytes) -> u16 {
    let [lead, high, low] = input.as_ref() else {
        return u16::MAX;
    };
    u16::from_be_bytes([*high, *low]) ^ u16::from(*lead)
}

#[test]
fn owned_decode_pair_is_semantically_equivalent() {
    for input in [Bytes::new(), Bytes::from_static(&[7, 0x12, 0x34])] {
        assert_eq!(
            owned_generated_fixed_decode(black_box(input.clone())),
            owned_handwritten_fixed_decode(black_box(input))
        );
    }
}
