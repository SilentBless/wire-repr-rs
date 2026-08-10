//! Release-codegen regression probes for generated fixed layouts.

use core::hint::black_box;
use wire_repr::wire_repr;

wire_repr! {
    /// A compact fixed layout used only by the release-codegen gate.
    pub layout CodegenPacket {
        /// The big-endian word under test.
        field word: BeU16 {
            position: 1;
            projections {
                bit word_low: 0;
            }
        }
    }
}

/// Generated fixed getter probe.
#[inline(never)]
pub fn generated_fixed_getter(bytes: &[u8]) -> Option<u16> {
    CodegenPacketView::parse_exact(bytes)
        .ok()
        .map(|view| view.word())
}

/// Equivalent handwritten fixed getter probe.
#[inline(never)]
pub fn handwritten_fixed_getter(bytes: &[u8]) -> Option<u16> {
    let bytes: &[u8; 2] = bytes.try_into().ok()?;
    Some(u16::from_be_bytes(*bytes))
}

/// Generated projection probe.
#[inline(never)]
pub fn generated_projection(bytes: &[u8]) -> Option<bool> {
    CodegenPacketView::parse_exact(bytes)
        .ok()
        .map(|view| view.word_low())
}

/// Equivalent handwritten projection probe.
#[inline(never)]
pub fn handwritten_projection(bytes: &[u8]) -> Option<bool> {
    let bytes: &[u8; 2] = bytes.try_into().ok()?;
    Some(u16::from_be_bytes(*bytes) & 1 != 0)
}

/// Generated same-width mutation probe.
#[inline(never)]
pub fn generated_mutation(bytes: &mut [u8], value: u16) -> bool {
    match CodegenPacketViewMut::parse_exact_mut(bytes) {
        Ok(mut view) => view.set_word(value).is_ok(),
        Err(_) => false,
    }
}

/// Equivalent handwritten same-width mutation probe.
#[inline(never)]
pub fn handwritten_mutation(bytes: &mut [u8], value: u16) -> bool {
    if bytes.len() == 2 {
        bytes.copy_from_slice(&value.to_be_bytes());
        true
    } else {
        false
    }
}

/// Generated fixed builder probe, including its short-output result.
#[inline(never)]
pub fn generated_builder(output: &mut [u8], value: u16) -> bool {
    CodegenPacketBuilder::new()
        .word(value)
        .build_into(output)
        .is_ok()
}

/// Equivalent handwritten fixed builder probe, including its short-output result.
#[inline(never)]
pub fn handwritten_builder(output: &mut [u8], value: u16) -> bool {
    if output.len() < 2 {
        false
    } else {
        output[..2].copy_from_slice(&value.to_be_bytes());
        true
    }
}

#[test]
fn generated_probes_match_handwritten_safe_rust() {
    let input = [0x12, 0x35];
    assert_eq!(
        black_box(generated_fixed_getter(black_box(&input))),
        black_box(handwritten_fixed_getter(black_box(&input)))
    );
    assert_eq!(
        black_box(generated_projection(black_box(&input))),
        black_box(handwritten_projection(black_box(&input)))
    );

    let mut generated_bytes = input;
    let mut handwritten_bytes = input;
    assert_eq!(
        black_box(generated_mutation(
            black_box(&mut generated_bytes),
            black_box(0xabcd)
        )),
        black_box(handwritten_mutation(
            black_box(&mut handwritten_bytes),
            black_box(0xabcd)
        ))
    );
    assert_eq!(generated_bytes, handwritten_bytes);

    for invalid in [&[][..], &[0x12][..], &[0x12, 0x34, 0x56][..]] {
        assert_eq!(
            generated_fixed_getter(invalid),
            handwritten_fixed_getter(invalid)
        );
        assert_eq!(
            generated_projection(invalid),
            handwritten_projection(invalid)
        );
    }

    let mut generated_short_mutation = [0x55];
    let mut handwritten_short_mutation = [0x55];
    assert_eq!(
        generated_mutation(&mut generated_short_mutation, 0xabcd),
        handwritten_mutation(&mut handwritten_short_mutation, 0xabcd)
    );
    assert_eq!(generated_short_mutation, handwritten_short_mutation);

    let mut generated_output = [0; 3];
    let mut handwritten_output = [0; 3];
    assert_eq!(
        black_box(generated_builder(
            black_box(&mut generated_output),
            black_box(0x1234)
        )),
        black_box(handwritten_builder(
            black_box(&mut handwritten_output),
            black_box(0x1234)
        ))
    );
    assert_eq!(generated_output, handwritten_output);

    let mut generated_short = [0x55];
    let mut handwritten_short = [0x55];
    assert_eq!(
        black_box(generated_builder(
            black_box(&mut generated_short),
            black_box(0x1234)
        )),
        black_box(handwritten_builder(
            black_box(&mut handwritten_short),
            black_box(0x1234)
        ))
    );
    assert_eq!(generated_short, handwritten_short);
}
