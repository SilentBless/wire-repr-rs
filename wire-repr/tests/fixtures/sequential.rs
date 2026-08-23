//! pair: fixed_decode = sequential_generated_fixed_decode / sequential_handwritten_fixed_decode
//! pair: fixed_encode = sequential_generated_fixed_encode / sequential_handwritten_fixed_encode
//! pair: bounded_decode = sequential_generated_bounded_decode / sequential_handwritten_bounded_decode
//! pair: fixed_sequence = sequential_generated_fixed_sequence / sequential_handwritten_fixed_sequence
//! pair: variable_cursor = sequential_generated_variable_cursor / sequential_handwritten_variable_cursor
//! tolerance: 160%
//! weights: instructions=1, branches=4, calls=8, panic_paths=16

#![allow(dead_code)]

use core::hint::black_box;
use wire_repr::Wire;

#[derive(Wire)]
struct FixedPacket {
    #[wire(be)]
    word: u16,
}

#[derive(Wire)]
struct DynamicPacket<'wire> {
    length: u8,
    #[wire(bytes = length)]
    payload: &'wire [u8],
    tail: u8,
}

#[inline(never)]
pub fn sequential_generated_fixed_decode(bytes: &[u8]) -> u16 {
    FixedPacket::view(bytes)
        .with_remainder()
        .map_or(u16::MAX, |(packet, _)| packet.word())
}

#[inline(never)]
pub fn sequential_handwritten_fixed_decode(bytes: &[u8]) -> u16 {
    let Some(bytes) = bytes.get(..2) else {
        return u16::MAX;
    };
    u16::from_be_bytes([bytes[0], bytes[1]])
}

#[inline(never)]
pub fn sequential_generated_fixed_encode(word: u16, output: &mut [u8]) -> usize {
    FixedPacket { word }
        .build_into(output)
        .map_or(0, |(written, _)| written.as_bytes().len())
}

#[inline(never)]
pub fn sequential_handwritten_fixed_encode(word: u16, output: &mut [u8]) -> usize {
    if output.len() < 2 {
        return 0;
    }
    output[..2].copy_from_slice(&word.to_be_bytes());
    2
}

#[inline(never)]
pub fn sequential_generated_bounded_decode(bytes: &[u8]) -> u8 {
    DynamicPacket::view(bytes)
        .without_trailing()
        .ok()
        .and_then(|packet| {
            packet
                .payload()
                .first()
                .copied()
                .map(|first| first ^ packet.tail() ^ packet.length())
        })
        .unwrap_or(u8::MAX)
}

#[inline(never)]
pub fn sequential_handwritten_bounded_decode(bytes: &[u8]) -> u8 {
    let Some((&length, remaining)) = bytes.split_first() else {
        return u8::MAX;
    };
    let length = usize::from(length);
    if remaining.len() != length + 1 || length == 0 {
        return u8::MAX;
    }
    remaining[0] ^ remaining[length] ^ length as u8
}

#[inline(never)]
pub fn sequential_generated_fixed_sequence(bytes: &[u8]) -> u16 {
    let Ok(packets) = FixedPacket::views(bytes) else {
        return u16::MAX;
    };
    packets.fold(0, |sum, packet| sum.wrapping_add(packet.word()))
}

#[inline(never)]
pub fn sequential_handwritten_fixed_sequence(bytes: &[u8]) -> u16 {
    let chunks = bytes.chunks_exact(2);
    if !chunks.remainder().is_empty() {
        return u16::MAX;
    }
    chunks.fold(0, |sum, bytes| {
        sum.wrapping_add(u16::from_be_bytes([bytes[0], bytes[1]]))
    })
}

#[inline(never)]
pub fn sequential_generated_variable_cursor(bytes: &[u8]) -> u8 {
    let mut records = DynamicPacket::cursor(bytes);
    let mut sum = 0u8;
    loop {
        match records.next() {
            Ok(Some(record)) => {
                sum = sum
                    .wrapping_add(record.length())
                    .wrapping_add(record.tail())
            }
            Ok(None) => return sum,
            Err(_) => return u8::MAX,
        }
    }
}

#[inline(never)]
pub fn sequential_handwritten_variable_cursor(mut bytes: &[u8]) -> u8 {
    let mut sum = 0u8;
    while let Some((&length, remaining)) = bytes.split_first() {
        let length = usize::from(length);
        let Some((&tail, suffix)) = remaining.get(length).zip(remaining.get(length + 1..)) else {
            return u8::MAX;
        };
        sum = sum.wrapping_add(length as u8).wrapping_add(tail);
        bytes = suffix;
    }
    sum
}

#[test]
fn sequential_pairs_are_semantically_equivalent() {
    for bytes in [&[][..], &[0x12], &[0x12, 0x34], &[0x12, 0x34, 0]] {
        assert_eq!(
            sequential_generated_fixed_decode(black_box(bytes)),
            sequential_handwritten_fixed_decode(black_box(bytes))
        );
    }
    for length in [0, 1, 2, 3] {
        let mut generated = [0xa5; 3];
        let mut handwritten = generated;
        assert_eq!(
            sequential_generated_fixed_encode(0x1234, black_box(&mut generated[..length])),
            sequential_handwritten_fixed_encode(0x1234, black_box(&mut handwritten[..length]))
        );
        assert_eq!(generated, handwritten);
    }
    for bytes in [&[][..], &[2, 0xaa, 0xbb, 0x55], &[0, 0x55], &[1, 0xaa]] {
        assert_eq!(
            sequential_generated_bounded_decode(black_box(bytes)),
            sequential_handwritten_bounded_decode(black_box(bytes))
        );
    }
    for bytes in [&[][..], &[0x12], &[0x12, 0x34], &[0x12, 0x34, 0xab, 0xcd]] {
        assert_eq!(
            sequential_generated_fixed_sequence(black_box(bytes)),
            sequential_handwritten_fixed_sequence(black_box(bytes))
        );
    }
    for bytes in [&[][..], &[0, 1], &[1, 9, 2], &[1, 9, 2, 0, 3], &[2, 9]] {
        assert_eq!(
            sequential_generated_variable_cursor(black_box(bytes)),
            sequential_handwritten_variable_cursor(black_box(bytes))
        );
    }
}
