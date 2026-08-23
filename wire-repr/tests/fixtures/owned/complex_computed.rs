//! Owned multi-computed append prepares dependencies before an atomic capacity preflight.
//! mode: bytes
//! pair: multi_computed_append = owned_complex_generated_append / owned_complex_handwritten_append
//! tolerance: 10%
//! weights: instructions=1, branches=4, calls=8, panic_paths=16

#![allow(dead_code)]

use bytes::BytesMut;
use core::hint::black_box;
use wire_repr::{ByteSourceCursor, Wire};

fn byte_sum(source: &impl ByteSourceCursor) -> u8 {
    source.bytes().fold(0, u8::wrapping_add)
}

#[derive(Wire)]
struct MultiComputed<'wire> {
    #[wire(computed = byte_sum(include(partial, payload)))]
    checksum: u8,
    #[wire(computed = byte_sum(include(payload)))]
    partial: u8,
    #[wire(rest)]
    payload: &'wire [u8],
}

#[inline(never)]
pub fn owned_complex_generated_append(payload: &[u8], output: &mut BytesMut) -> usize {
    MultiComputed::builder()
        .payload(payload)
        .build_into(output)
        .map_or(0, |written| written.as_bytes().len())
}

#[inline(never)]
pub fn owned_complex_handwritten_append(payload: &[u8], output: &mut BytesMut) -> usize {
    let Some(required) = payload.len().checked_add(2) else {
        return 0;
    };
    let partial = payload.iter().copied().fold(0, u8::wrapping_add);
    let checksum = payload.iter().copied().fold(partial, u8::wrapping_add);
    if output.capacity() - output.len() < required {
        return 0;
    }

    output.extend_from_slice(&[checksum, partial]);
    output.extend_from_slice(payload);
    required
}

#[test]
fn owned_complex_append_pair_preserves_atomic_no_growth_contract() {
    for (prefix, payload, capacity, expected_success) in [
        (&[][..], &[][..], 2, true),
        (&[0xaa, 0xbb][..], &[4, 5, 6][..], 7, true),
        (&[0xaa][..], &[4, 5][..], 4, false),
        (&[0xaa][..], &[4, 5][..], 5, true),
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
        let required = payload.len() + 2;
        let has_capacity = generated_capacity - generated.len() >= required;
        assert_eq!(has_capacity, expected_success);
        assert_eq!(
            has_capacity,
            handwritten_capacity - handwritten.len() >= required
        );

        let generated_written = owned_complex_generated_append(payload, black_box(&mut generated));
        let handwritten_written =
            owned_complex_handwritten_append(payload, black_box(&mut handwritten));
        assert_eq!(generated_written, handwritten_written);
        assert_eq!(
            generated_written,
            if expected_success { required } else { 0 }
        );
        assert_eq!(generated.as_ptr(), generated_pointer);
        assert_eq!(handwritten.as_ptr(), handwritten_pointer);
        assert_eq!(generated.capacity(), generated_capacity);
        assert_eq!(handwritten.capacity(), handwritten_capacity);
        assert_eq!(generated, handwritten);

        if has_capacity {
            let partial = payload.iter().copied().fold(0, u8::wrapping_add);
            let checksum = payload.iter().copied().fold(partial, u8::wrapping_add);
            let mut expected = prefix.to_vec();
            expected.extend_from_slice(&[checksum, partial]);
            expected.extend_from_slice(payload);
            assert_eq!(generated.as_ref(), expected);
        } else {
            assert_eq!(generated, generated_before);
            assert_eq!(handwritten, handwritten_before);
        }
    }
}
