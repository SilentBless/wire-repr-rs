//! Owned output pays for checked planning and atomic capacity preflight.
//! mode: bytes
//! pair: dynamic = owned_generated_append / owned_handwritten_append
//! tolerance: 30%
//! weights: instructions=1, branches=4, calls=8, panic_paths=16

#![allow(dead_code)]

use bytes::BytesMut;
use core::hint::black_box;
use wire_repr::Wire;

#[derive(Wire)]
struct Dynamic<'wire> {
    length: u8,
    #[wire(bytes = length)]
    payload: &'wire [u8],
}

#[inline(never)]
pub fn owned_generated_append(payload: &[u8], output: &mut BytesMut) -> usize {
    Dynamic::builder()
        .payload(payload)
        .build_into(output)
        .map_or(0, |written| written.as_bytes().len())
}

#[inline(never)]
pub fn owned_handwritten_append(payload: &[u8], output: &mut BytesMut) -> usize {
    let Ok(length) = u8::try_from(payload.len()) else {
        return 0;
    };
    let required = usize::from(length) + 1;
    if output.capacity() - output.len() < required {
        return 0;
    }
    output.extend_from_slice(&[length]);
    output.extend_from_slice(payload);
    required
}

#[test]
fn owned_append_pair_is_semantically_equivalent_without_growth() {
    for capacity in [2, 4] {
        let mut generated = BytesMut::with_capacity(capacity);
        let mut handwritten = BytesMut::with_capacity(capacity);
        generated.extend_from_slice(&[0xaa]);
        handwritten.extend_from_slice(&[0xaa]);
        assert_eq!(
            owned_generated_append(&[4, 5], black_box(&mut generated)),
            owned_handwritten_append(&[4, 5], black_box(&mut handwritten))
        );
        assert_eq!(generated, handwritten);
    }
}
