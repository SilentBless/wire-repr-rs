//! pair: dependency = complex_computed_generated_dependency / complex_computed_handwritten_dependency
//! pair: callback = complex_computed_generated_callback / complex_computed_handwritten_callback
//! pair: nested = complex_computed_generated_nested / complex_computed_handwritten_nested
//! tolerance: 10%
//! weights: instructions=1, branches=4, calls=8, panic_paths=16

#![allow(dead_code)]

use core::hint::black_box;
use wire_repr::{ByteSourceCursor, Wire};

fn byte_sum(source: &impl ByteSourceCursor) -> u8 {
    source.bytes().fold(0u8, u8::wrapping_add)
}

fn ordered_count(
    kind: &u8,
    first: &impl ByteSourceCursor,
    remaining: &impl ByteSourceCursor,
) -> usize {
    usize::from(*kind) * 100 + first.byte_len() * 10 + remaining.byte_len()
}

#[derive(Wire)]
struct DependencyPacket<'wire> {
    #[wire(computed = byte_sum(include(partial, payload)))]
    checksum: u8,
    #[wire(computed = byte_sum(include(payload)))]
    partial: u8,
    #[wire(rest)]
    payload: &'wire [u8],
}

#[derive(Wire)]
struct CallbackPacket {
    #[wire(computed = ordered_count(kind, include(first), exclude(second)))]
    checksum: u8,
    kind: u8,
    first: u8,
    second: u8,
    tail: u8,
}

#[derive(Wire)]
struct NestedTail<'wire> {
    kind: u8,
    #[wire(rest)]
    data: &'wire [u8],
}

#[derive(Wire)]
struct NestedPacket<'wire> {
    #[wire(computed = byte_sum(include(tail.kind)))]
    checksum: u8,
    tail: NestedTail<'wire>,
}

#[inline(never)]
pub fn complex_computed_generated_dependency(payload: &[u8], output: &mut [u8]) -> usize {
    DependencyPacket::builder()
        .payload(payload)
        .build_into(output)
        .map_or(0, |(written, _)| written.as_bytes().len())
}

#[inline(never)]
pub fn complex_computed_handwritten_dependency(payload: &[u8], output: &mut [u8]) -> usize {
    let required = payload.len() + 2;
    if output.len() < required {
        return 0;
    }
    let partial = payload.iter().copied().fold(0, u8::wrapping_add);
    let checksum = payload
        .iter()
        .copied()
        .fold(partial, u8::wrapping_add);
    output[0] = checksum;
    output[1] = partial;
    output[2..required].copy_from_slice(payload);
    required
}

#[inline(never)]
pub fn complex_computed_generated_callback(kind: u8, output: &mut [u8]) -> usize {
    CallbackPacket::builder()
        .kind(kind)
        .first(7)
        .second(8)
        .tail(9)
        .build_into(output)
        .map_or(0, |(written, _)| written.as_bytes().len())
}

#[inline(never)]
pub fn complex_computed_handwritten_callback(kind: u8, output: &mut [u8]) -> usize {
    // `exclude(second)` retains the computed destination, first, and tail: three bytes.
    let Ok(checksum) = u8::try_from(usize::from(kind) * 100 + 10 + 3) else {
        return 0;
    };
    if output.len() < 5 {
        return 0;
    }
    output[..5].copy_from_slice(&[checksum, kind, 7, 8, 9]);
    5
}

#[inline(never)]
pub fn complex_computed_generated_nested(payload: &[u8], output: &mut [u8]) -> usize {
    NestedPacket::builder()
        .tail(NestedTail {
            kind: 7,
            data: payload,
        })
        .build_into(output)
        .map_or(0, |(written, _)| written.as_bytes().len())
}

#[inline(never)]
pub fn complex_computed_handwritten_nested(payload: &[u8], output: &mut [u8]) -> usize {
    let required = payload.len() + 2;
    if output.len() < required {
        return 0;
    }
    output[0] = 7;
    output[1] = 7;
    output[2..required].copy_from_slice(payload);
    required
}

#[test]
fn complex_computed_pairs_are_semantically_equivalent() {
    for payload in [&[][..], &[1, 2][..], &[200, 100, 60][..]] {
        let required = payload.len() + 2;
        for length in [0, required.saturating_sub(1), required, required + 3] {
            let mut generated = [0xa5; 8];
            let mut handwritten = generated;
            assert_eq!(
                complex_computed_generated_dependency(payload, black_box(&mut generated[..length])),
                complex_computed_handwritten_dependency(payload, black_box(&mut handwritten[..length]))
            );
            assert_eq!(generated, handwritten);

            let mut generated = [0xa5; 8];
            let mut handwritten = generated;
            assert_eq!(
                complex_computed_generated_nested(payload, black_box(&mut generated[..length])),
                complex_computed_handwritten_nested(payload, black_box(&mut handwritten[..length]))
            );
            assert_eq!(generated, handwritten);
        }
    }

    for kind in [2, 3] {
        for length in [0, 4, 5, 8] {
            let mut generated = [0xa5; 8];
            let mut handwritten = generated;
            assert_eq!(
                complex_computed_generated_callback(kind, black_box(&mut generated[..length])),
                complex_computed_handwritten_callback(kind, black_box(&mut handwritten[..length]))
            );
            assert_eq!(generated, handwritten);
        }
    }
}
