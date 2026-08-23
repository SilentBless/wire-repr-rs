//! pair: computed_position_encode = complex_geometry_generated_computed_position_encode / complex_geometry_handwritten_computed_position_encode
//! pair: cross_referenced_decode = complex_geometry_generated_cross_referenced_decode / complex_geometry_handwritten_cross_referenced_decode
//! tolerance: 10%
//! weights: instructions=1, branches=4, calls=8, panic_paths=16

#![allow(dead_code)]

use core::hint::black_box;
use wire_repr::Wire;

fn semantic_offset(lead: &u8) -> usize {
    usize::from(*lead)
}

#[derive(Wire)]
struct ComputedPosition {
    lead: u8,
    #[wire(computed = semantic_offset(lead))]
    payload_offset: u8,
    #[wire(at = payload_offset)]
    payload: u8,
}

#[derive(Wire)]
struct Positioned<'wire> {
    payload_offset: u8,
    payload_length: u8,
    lead: u8,
    #[wire(at = payload_offset, bytes = payload_length)]
    payload: &'wire [u8],
    tail: u8,
}

#[inline(never)]
pub fn complex_geometry_generated_computed_position_encode(
    lead: u8,
    payload: u8,
    output: &mut [u8],
) -> usize {
    ComputedPosition::builder()
        .lead(lead)
        .payload(payload)
        .build_into(output)
        .map_or(0, |(written, _)| written.as_bytes().len())
}

#[inline(never)]
pub fn complex_geometry_handwritten_computed_position_encode(
    lead: u8,
    payload: u8,
    output: &mut [u8],
) -> usize {
    let payload_offset = match u8::try_from(semantic_offset(&lead)) {
        Ok(payload_offset) => payload_offset,
        Err(_) => return 0,
    };
    let payload_offset = usize::from(payload_offset);
    if payload_offset < 2 || output.len() < payload_offset + 1 {
        return 0;
    }
    output[0] = lead;
    output[1] = payload_offset as u8;
    output[2..payload_offset].fill(0);
    output[payload_offset] = payload;
    payload_offset + 1
}

#[inline(never)]
pub fn complex_geometry_generated_cross_referenced_decode(bytes: &[u8]) -> usize {
    Positioned::view(bytes)
        .without_trailing()
        .map_or(usize::MAX, |frame| {
            (frame.as_bytes().len() << 8)
                | usize::from(
                    frame.lead()
                        ^ frame.tail()
                        ^ frame.payload().iter().fold(0, |sum, byte| sum ^ byte),
                )
        })
}

#[inline(never)]
pub fn complex_geometry_handwritten_cross_referenced_decode(bytes: &[u8]) -> usize {
    let Some(header) = bytes.get(..3) else {
        return usize::MAX;
    };
    let payload_offset = usize::from(header[0]);
    let payload_length = usize::from(header[1]);
    let lead = header[2];
    if payload_offset < header.len() {
        return usize::MAX;
    }
    let Some((_, positioned)) = bytes.split_at_checked(payload_offset) else {
        return usize::MAX;
    };
    let Some((payload, after_payload)) = positioned.split_at_checked(payload_length) else {
        return usize::MAX;
    };
    let Some((&tail, suffix)) = after_payload.split_first() else {
        return usize::MAX;
    };
    if !suffix.is_empty() {
        return usize::MAX;
    }
    ((payload_offset + payload_length + 1) << 8)
        | usize::from(lead ^ tail ^ payload.iter().fold(0, |sum, byte| sum ^ byte))
}

#[test]
fn complex_geometry_pairs_are_semantically_equivalent() {
    for lead in [1, 4] {
        for length in [0, 4, 5, 8] {
            let mut generated = [0xa5; 8];
            let mut handwritten = generated;
            assert_eq!(
                complex_geometry_generated_computed_position_encode(
                    lead,
                    9,
                    black_box(&mut generated[..length]),
                ),
                complex_geometry_handwritten_computed_position_encode(
                    lead,
                    9,
                    black_box(&mut handwritten[..length]),
                ),
            );
            assert_eq!(generated, handwritten);
        }
    }

    for bytes in [
        &[][..],
        &[4],
        &[4, 1],
        &[2, 0, 9],
        &[4, 2, 9, 0xaa, 0xbb],
        &[4, 1, 9, 0xaa],
        &[4, 1, 9, 0, 0xaa, 0xbb],
        &[4, 1, 9, 0, 0xaa, 0xbb, 0xcc],
    ] {
        assert_eq!(
            complex_geometry_generated_cross_referenced_decode(black_box(bytes)),
            complex_geometry_handwritten_cross_referenced_decode(black_box(bytes)),
        );
    }

    let exact = [4, 1, 9, 0, 0xaa, 0xbb];
    let (frame, suffix) = Positioned::view(&exact).with_remainder().unwrap();
    assert_eq!(frame.as_bytes().len(), 6);
    assert!(suffix.is_empty());
}
