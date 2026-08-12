//! Release-codegen regression probes for generated fixed layouts.

use core::hint::black_box;
use wire_repr::wire_repr;

/// A total nominal mapping used only by the release-codegen gate.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct CodegenMapped(u32);

impl From<u32> for CodegenMapped {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<CodegenMapped> for u32 {
    fn from(value: CodegenMapped) -> Self {
        value.0
    }
}

wire_repr! {
    /// A nominal big-endian word used only by the release-codegen gate.
    pub scalar CodegenHardwareType: LeU16;

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

    /// A compact nominal-scalar layout used only by the release-codegen gate.
    pub layout CodegenScalarPacket {
        /// The nominal big-endian word under test.
        field hardware_type: CodegenHardwareType;
    }

    /// A compact total-mapped layout used only by the release-codegen gate.
    pub layout CodegenMappedPacket {
        /// The mapped big-endian word under test.
        field mapped: BeU32 as crate::CodegenMapped;
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

/// Generated declared-scalar getter probe.
#[inline(never)]
pub fn generated_scalar_getter(bytes: &[u8]) -> Option<u16> {
    CodegenScalarPacketView::parse_exact(bytes)
        .ok()
        .map(|view| view.hardware_type().raw())
}

/// Equivalent handwritten declared-scalar getter probe.
#[inline(never)]
pub fn handwritten_scalar_getter(bytes: &[u8]) -> Option<u16> {
    let bytes: &[u8; 2] = bytes.try_into().ok()?;
    Some(u16::from_le_bytes(*bytes))
}

/// Generated declared-scalar same-width mutation probe.
#[inline(never)]
pub fn generated_scalar_mutation(bytes: &mut [u8], value: u16) -> bool {
    match CodegenScalarPacketViewMut::parse_exact_mut(bytes) {
        Ok(mut view) => view
            .set_hardware_type(CodegenHardwareType::new(value))
            .is_ok(),
        Err(_) => false,
    }
}

/// Equivalent handwritten declared-scalar same-width mutation probe.
#[inline(never)]
pub fn handwritten_scalar_mutation(bytes: &mut [u8], value: u16) -> bool {
    if bytes.len() == 2 {
        bytes.copy_from_slice(&value.to_le_bytes());
        true
    } else {
        false
    }
}

/// Generated declared-scalar builder probe, including its short-output result.
#[inline(never)]
pub fn generated_scalar_builder(output: &mut [u8], value: u16) -> bool {
    CodegenScalarPacketBuilder::new()
        .hardware_type(CodegenHardwareType::new(value))
        .build_into(output)
        .is_ok()
}

/// Equivalent handwritten declared-scalar builder probe, including its short-output result.
#[inline(never)]
pub fn handwritten_scalar_builder(output: &mut [u8], value: u16) -> bool {
    if output.len() < 2 {
        false
    } else {
        output[..2].copy_from_slice(&value.to_le_bytes());
        true
    }
}

/// Generated total-mapped getter probe.
#[inline(never)]
pub fn generated_mapped_getter(bytes: &[u8]) -> Option<u32> {
    CodegenMappedPacketView::parse_exact(bytes)
        .ok()
        .map(|view| view.mapped().into())
}

/// Equivalent handwritten total-mapped getter probe.
#[inline(never)]
pub fn handwritten_mapped_getter(bytes: &[u8]) -> Option<u32> {
    let bytes: &[u8; 4] = bytes.try_into().ok()?;
    Some(u32::from_be_bytes(*bytes))
}

/// Generated total-mapped same-width mutation probe.
#[inline(never)]
pub fn generated_mapped_mutation(bytes: &mut [u8], value: u32) -> bool {
    match CodegenMappedPacketViewMut::parse_exact_mut(bytes) {
        Ok(mut view) => view.set_mapped(value.into()).is_ok(),
        Err(_) => false,
    }
}

/// Equivalent handwritten total-mapped same-width mutation probe.
#[inline(never)]
pub fn handwritten_mapped_mutation(bytes: &mut [u8], value: u32) -> bool {
    if bytes.len() == 4 {
        bytes.copy_from_slice(&value.to_be_bytes());
        true
    } else {
        false
    }
}

/// Generated total-mapped builder probe, including its short-output result.
#[inline(never)]
pub fn generated_mapped_builder(output: &mut [u8], value: u32) -> bool {
    CodegenMappedPacketBuilder::new()
        .mapped(value.into())
        .build_into(output)
        .is_ok()
}

/// Equivalent handwritten total-mapped builder probe, including its short-output result.
#[inline(never)]
pub fn handwritten_mapped_builder(output: &mut [u8], value: u32) -> bool {
    if output.len() < 4 {
        false
    } else {
        output[..4].copy_from_slice(&value.to_be_bytes());
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
    assert_eq!(
        black_box(generated_scalar_getter(black_box(&input))),
        black_box(handwritten_scalar_getter(black_box(&input)))
    );
    let mapped_input = [0x12, 0x35, 0x67, 0x89];
    assert_eq!(
        black_box(generated_mapped_getter(black_box(&mapped_input))),
        black_box(handwritten_mapped_getter(black_box(&mapped_input)))
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

    let mut generated_scalar_bytes = input;
    let mut handwritten_scalar_bytes = input;
    assert_eq!(
        black_box(generated_scalar_mutation(
            black_box(&mut generated_scalar_bytes),
            black_box(0xabcd)
        )),
        black_box(handwritten_scalar_mutation(
            black_box(&mut handwritten_scalar_bytes),
            black_box(0xabcd)
        ))
    );
    assert_eq!(generated_scalar_bytes, handwritten_scalar_bytes);

    let mut generated_mapped_bytes = mapped_input;
    let mut handwritten_mapped_bytes = mapped_input;
    assert_eq!(
        black_box(generated_mapped_mutation(
            black_box(&mut generated_mapped_bytes),
            black_box(0xabcd_ef01)
        )),
        black_box(handwritten_mapped_mutation(
            black_box(&mut handwritten_mapped_bytes),
            black_box(0xabcd_ef01)
        ))
    );
    assert_eq!(generated_mapped_bytes, handwritten_mapped_bytes);

    for invalid in [&[][..], &[0x12][..], &[0x12, 0x34, 0x56][..]] {
        assert_eq!(
            generated_fixed_getter(invalid),
            handwritten_fixed_getter(invalid)
        );
        assert_eq!(
            generated_projection(invalid),
            handwritten_projection(invalid)
        );
        assert_eq!(
            generated_scalar_getter(invalid),
            handwritten_scalar_getter(invalid)
        );
        assert_eq!(
            generated_mapped_getter(invalid),
            handwritten_mapped_getter(invalid)
        );
    }

    let mut generated_short_mutation = [0x55];
    let mut handwritten_short_mutation = [0x55];
    assert_eq!(
        generated_mutation(&mut generated_short_mutation, 0xabcd),
        handwritten_mutation(&mut handwritten_short_mutation, 0xabcd)
    );
    assert_eq!(generated_short_mutation, handwritten_short_mutation);

    let mut generated_scalar_short_mutation = [0x55];
    let mut handwritten_scalar_short_mutation = [0x55];
    assert_eq!(
        generated_scalar_mutation(&mut generated_scalar_short_mutation, 0xabcd),
        handwritten_scalar_mutation(&mut handwritten_scalar_short_mutation, 0xabcd)
    );
    assert_eq!(
        generated_scalar_short_mutation,
        handwritten_scalar_short_mutation
    );

    let mut generated_mapped_short_mutation = [0x55];
    let mut handwritten_mapped_short_mutation = [0x55];
    assert_eq!(
        generated_mapped_mutation(&mut generated_mapped_short_mutation, 0xabcd_ef01),
        handwritten_mapped_mutation(&mut handwritten_mapped_short_mutation, 0xabcd_ef01)
    );
    assert_eq!(
        generated_mapped_short_mutation,
        handwritten_mapped_short_mutation
    );

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

    let mut generated_scalar_output = [0; 3];
    let mut handwritten_scalar_output = [0; 3];
    assert_eq!(
        black_box(generated_scalar_builder(
            black_box(&mut generated_scalar_output),
            black_box(0x1234)
        )),
        black_box(handwritten_scalar_builder(
            black_box(&mut handwritten_scalar_output),
            black_box(0x1234)
        ))
    );
    assert_eq!(generated_scalar_output, handwritten_scalar_output);

    let mut generated_mapped_output = [0; 5];
    let mut handwritten_mapped_output = [0; 5];
    assert_eq!(
        black_box(generated_mapped_builder(
            black_box(&mut generated_mapped_output),
            black_box(0x1234_5678)
        )),
        black_box(handwritten_mapped_builder(
            black_box(&mut handwritten_mapped_output),
            black_box(0x1234_5678)
        ))
    );
    assert_eq!(generated_mapped_output, handwritten_mapped_output);

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

    let mut generated_scalar_short = [0x55];
    let mut handwritten_scalar_short = [0x55];
    assert_eq!(
        black_box(generated_scalar_builder(
            black_box(&mut generated_scalar_short),
            black_box(0x1234)
        )),
        black_box(handwritten_scalar_builder(
            black_box(&mut handwritten_scalar_short),
            black_box(0x1234)
        ))
    );
    assert_eq!(generated_scalar_short, handwritten_scalar_short);

    let mut generated_mapped_short = [0x55];
    let mut handwritten_mapped_short = [0x55];
    assert_eq!(
        black_box(generated_mapped_builder(
            black_box(&mut generated_mapped_short),
            black_box(0x1234_5678)
        )),
        black_box(handwritten_mapped_builder(
            black_box(&mut handwritten_mapped_short),
            black_box(0x1234_5678)
        ))
    );
    assert_eq!(generated_mapped_short, handwritten_mapped_short);
}
