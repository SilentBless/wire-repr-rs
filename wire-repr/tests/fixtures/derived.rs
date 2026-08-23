//! pair: encode = derived_generated_encode / derived_handwritten_encode
//! tolerance: 10%
//! weights: instructions=1, branches=4, calls=8, panic_paths=16

#![allow(dead_code)]

use core::hint::black_box;
use wire_repr::Wire;

#[derive(Wire)]
struct Packet<'wire> {
    #[wire(computed = wire_repr::computation::len(payload))]
    length: u8,
    kind: u8,
    #[wire(rest)]
    payload: &'wire [u8],
}

#[inline(never)]
pub fn derived_generated_encode(payload: &[u8], output: &mut [u8]) -> usize {
    Packet::builder()
        .kind(9)
        .payload(payload)
        .build_into(output)
        .map_or(0, |(written, _)| written.as_bytes().len())
}
#[inline(never)]
pub fn derived_handwritten_encode(payload: &[u8], output: &mut [u8]) -> usize {
    let Ok(length) = u8::try_from(payload.len()) else {
        return 0;
    };
    let required = usize::from(length) + 2;
    if output.len() < required {
        return 0;
    }
    output[0] = length;
    output[1] = 9;
    output[2..required].copy_from_slice(payload);
    required
}

#[test]
fn derived_pair_is_semantically_equivalent() {
    for (payload, length) in [(&[][..], 0), (&[4, 5][..], 3), (&[4, 5][..], 4)] {
        let mut generated = [0xa5; 8];
        let mut handwritten = generated;
        assert_eq!(
            derived_generated_encode(payload, black_box(&mut generated[..length])),
            derived_handwritten_encode(payload, black_box(&mut handwritten[..length]))
        );
        assert_eq!(generated, handwritten);
    }
}
