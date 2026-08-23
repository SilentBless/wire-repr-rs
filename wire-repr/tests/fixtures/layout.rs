//! pair: positioned_encode = layout_generated_positioned / layout_handwritten_positioned
//! pair: bitfield_decode = layout_generated_bitfield / layout_handwritten_bitfield
//! tolerance: 10%
//! weights: instructions=1, branches=4, calls=8, panic_paths=16

use core::hint::black_box;
use wire_repr::Wire;

#[derive(Wire)]
struct Positioned {
    tag: u8,
    #[wire(at = 4, be)]
    word: u16,
}

#[derive(Wire)]
#[wire(bitfield = u16, be, reserved = zero)]
struct Flags {
    #[wire(bit = 0)]
    enabled: bool,
    #[wire(bits = 1..=3)]
    mode: u8,
}

#[inline(never)]
pub fn layout_generated_positioned(word: u16, output: &mut [u8]) -> usize {
    Positioned { tag: 9, word }
        .build_into(output)
        .map_or(0, |(written, _)| written.as_bytes().len())
}
#[inline(never)]
pub fn layout_handwritten_positioned(word: u16, output: &mut [u8]) -> usize {
    if output.len() < 6 {
        return 0;
    }
    output[0] = 9;
    output[1..4].fill(0);
    output[4..6].copy_from_slice(&word.to_be_bytes());
    6
}
#[inline(never)]
pub fn layout_generated_bitfield(bytes: &[u8]) -> u8 {
    Flags::view(bytes)
        .without_trailing()
        .map_or(u8::MAX, |flags| {
            u8::from(flags.enabled()) | (flags.mode() << 1)
        })
}
#[inline(never)]
pub fn layout_handwritten_bitfield(bytes: &[u8]) -> u8 {
    let [high, low] = bytes else { return u8::MAX };
    let raw = u16::from_be_bytes([*high, *low]);
    u8::from(raw & 1 != 0) | (((raw >> 1) as u8 & 7) << 1)
}

#[test]
fn layout_pairs_are_semantically_equivalent() {
    for length in [0, 5, 6, 8] {
        let mut generated = [0xa5; 8];
        let mut handwritten = generated;
        assert_eq!(
            layout_generated_positioned(0x1234, black_box(&mut generated[..length])),
            layout_handwritten_positioned(0x1234, black_box(&mut handwritten[..length]))
        );
        assert_eq!(generated, handwritten);
    }
    for bytes in [&[][..], &[0], &[0, 0x0b], &[0xff, 0xff]] {
        assert_eq!(
            layout_generated_bitfield(black_box(bytes)),
            layout_handwritten_bitfield(black_box(bytes))
        );
    }
}
