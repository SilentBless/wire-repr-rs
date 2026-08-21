//! Release-codegen regression probes for generated layouts.

use core::hint::black_box;
use wire_repr::{FixedCodec, RangeSource, U8, wire_repr};

/// A structural conversion error used only by the release-codegen gate.
#[derive(Debug)]
pub enum CodegenWordsError {
    /// The source-to-byte multiplication overflowed.
    Arithmetic,
    /// The byte geometry cannot be represented as a whole number of words.
    Unaligned,
    /// The word count cannot be represented by the source byte.
    TooLarge,
}

/// A four-byte-word range source used only by the release-codegen gate.
pub struct CodegenWords;

impl RangeSource<U8> for CodegenWords {
    type Error = CodegenWordsError;

    fn to_bytes(value: <U8 as FixedCodec>::Value<'_>) -> Result<usize, Self::Error> {
        usize::from(value)
            .checked_mul(4)
            .ok_or(CodegenWordsError::Arithmetic)
    }

    fn from_bytes(bytes: usize) -> Result<<U8 as FixedCodec>::Value<'static>, Self::Error> {
        if !bytes.is_multiple_of(4) {
            return Err(CodegenWordsError::Unaligned);
        }
        u8::try_from(bytes / 4).map_err(|_| CodegenWordsError::TooLarge)
    }
}

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

fn codegen_derived_total(payload: usize) -> Result<u8, core::convert::Infallible> {
    Ok(payload as u8)
}

#[inline(always)]
fn codegen_finalized_sum(bytes: &[u8]) -> u16 {
    bytes
        .iter()
        .fold(0u16, |sum, byte| sum.wrapping_add(u16::from(*byte)))
}

wire_repr! {
    /// A nominal big-endian word used only by the release-codegen gate.
    pub scalar CodegenHardwareType: LeU16;

    /// A compact fixed layout used only by the release-codegen gate.
    pub layout CodegenPacket {
        /// The big-endian word under test.
        word @ 1: BeU16 {

            projections {
                bit word_low: 0;
            }
        };
    }

    /// A sparse fixed absolute layout used only by the release-codegen gate.
    pub absolute layout CodegenAbsolutePacket {
        /// The trailing big-endian word.
        word @ 3: BeU16;
        /// The leading byte.
        head @ 0: U8;
    }

    /// A compact nominal-scalar layout used only by the release-codegen gate.
    pub layout CodegenScalarPacket {
        /// The nominal big-endian word under test.
        hardware_type: CodegenHardwareType;
    }

    /// A compact total-mapped layout used only by the release-codegen gate.
    pub layout CodegenMappedPacket {
        /// The mapped big-endian word under test.
        mapped: BeU32 as crate::CodegenMapped;
    }

    /// A relative byte range used only by the release-codegen gate.
    pub layout CodegenRelativeRange {
        /// The raw range length under test.
        length: U8;
        /// Bytes ending at the current position plus `length`.
        payload: bytes(length);
    }

    /// An absolute byte range used only by the release-codegen gate.
    pub layout CodegenAbsoluteRange {
        /// The exclusive endpoint from representation byte zero.
        end: U8;
        /// Bytes ending at `end`.
        payload: bytes_to(end);
    }

    /// A derived dynamic builder used only by the release-codegen gate.
    pub layout CodegenDerivedRange {
        tag: U8;
        length: U8;
        payload: bytes(length);
        total: U8 {
            derive: crate::codegen_derived_total(len(payload));
            derive_error: core::convert::Infallible;
        };
    }

    /// A prepared dynamic range layout used only by the release-codegen gate.
    pub layout CodegenPreparedDynamic {
        tag @ 1: U8;
        length @ 2: U8;
        payload @ 3: bytes(length);
        padding(1) @ 4;
        align(4) @ 5;
    }

    /// A word-counted dynamic layout used only by the release-codegen gate.
    pub layout CodegenTransformedRange {
        /// The payload length measured in four-byte words.
        length: U8 { range_source: crate::CodegenWords; };
        /// The payload whose byte length is derived from `length`.
        payload: bytes(length);
    }

    /// A post-write-finalized layout used only by the release-codegen gate.
    pub layout CodegenFinalizedPacket {
        /// The ordinary caller-supplied byte.
        tag: U8;
        /// The big-endian post-write finalizer target.
        checksum: BeU16 {
            finalize: crate::codegen_finalized_sum(bytes(buf_start..buf_end));
        };
    }

    /// A terminal byte range used only by the release-codegen gate.
    pub layout CodegenTerminalRange {
        /// The fixed header byte under test.
        header: U8;
        /// Every caller-bounded byte after the header.
        payload: remaining_bytes;
    }
}

/// Generated fixed getter probe.
#[inline(never)]
pub fn generated_fixed_getter(bytes: &[u8]) -> Option<u16> {
    CodegenPacket::view(bytes)
        .without_trailing()
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
    CodegenPacket::view(bytes)
        .without_trailing()
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

/// Generated explicit prepared-layout commit probe.
#[inline(never)]
pub fn generated_prepared_builder(output: &mut [u8], value: u16) -> (bool, usize) {
    let Ok(plan) = CodegenPacketBuilder::new().word(value).prepare() else {
        return (false, 0);
    };
    let encoded_len = plan.encoded_len();
    (plan.commit_into(output).is_ok(), encoded_len)
}

/// Equivalent handwritten prepared-layout commit probe.
#[inline(never)]
pub fn handwritten_prepared_builder(output: &mut [u8], value: u16) -> (bool, usize) {
    if output.len() < 2 {
        (false, 2)
    } else {
        output[..2].copy_from_slice(&value.to_be_bytes());
        (true, 2)
    }
}

/// Generated dynamic prepared-layout probe with derived range geometry.
#[inline(never)]
pub fn generated_dynamic_prepared_builder(
    output: &mut [u8],
    tag: u8,
    payload: &[u8],
) -> (bool, usize) {
    let Ok(plan) = CodegenPreparedDynamicBuilder::new()
        .tag(tag)
        .payload(payload)
        .prepare()
    else {
        return (false, 0);
    };
    let encoded_len = plan.encoded_len();
    (plan.commit_into(output).is_ok(), encoded_len)
}

/// Equivalent handwritten dynamic prepared-layout probe.
#[inline(never)]
pub fn handwritten_dynamic_prepared_builder(
    output: &mut [u8],
    tag: u8,
    payload: &[u8],
) -> (bool, usize) {
    let Ok(length) = u8::try_from(payload.len()) else {
        return (false, 0);
    };
    let Some(after_padding) = payload.len().checked_add(3) else {
        return (false, 0);
    };
    let padding = (4 - after_padding % 4) % 4;
    let Some(encoded_len) = after_padding.checked_add(padding) else {
        return (false, 0);
    };
    if output.len() < encoded_len {
        return (false, encoded_len);
    }
    output[0] = tag;
    output[1] = length;
    output[2..2 + payload.len()].copy_from_slice(payload);
    (true, encoded_len)
}

/// Generated transformed-range parser and payload getter probe.
#[inline(never)]
pub fn generated_transformed_range_getter(bytes: &[u8]) -> Option<u8> {
    CodegenTransformedRange::view(bytes)
        .without_trailing()
        .ok()?
        .payload()
        .first()
        .copied()
}

/// Equivalent handwritten transformed-range parser and payload getter probe.
#[inline(never)]
pub fn handwritten_transformed_range_getter(bytes: &[u8]) -> Option<u8> {
    let (&words, payload) = bytes.split_first()?;
    let expected = usize::from(words).checked_mul(4)?;
    if payload.len() != expected {
        return None;
    }
    payload.first().copied()
}

/// Generated transformed-range prepared-layout commit probe.
#[inline(never)]
pub fn generated_transformed_prepared_builder(output: &mut [u8], payload: &[u8]) -> (bool, usize) {
    let Ok(plan) = CodegenTransformedRangeBuilder::new()
        .payload(payload)
        .prepare()
    else {
        return (false, 0);
    };
    let encoded_len = plan.encoded_len();
    (plan.commit_into(output).is_ok(), encoded_len)
}

/// Equivalent handwritten transformed-range prepared-layout commit probe.
#[inline(never)]
pub fn handwritten_transformed_prepared_builder(
    output: &mut [u8],
    payload: &[u8],
) -> (bool, usize) {
    if !payload.len().is_multiple_of(4) {
        return (false, 0);
    }
    let Ok(words) = u8::try_from(payload.len() / 4) else {
        return (false, 0);
    };
    let Some(encoded_len) = payload.len().checked_add(1) else {
        return (false, 0);
    };
    if output.len() < encoded_len {
        return (false, encoded_len);
    }
    output[0] = words;
    output[1..encoded_len].copy_from_slice(payload);
    (true, encoded_len)
}

/// Generated fixed-absolute prepared-layout commit probe.
#[inline(never)]
pub fn generated_absolute_prepared_builder(output: &mut [u8], head: u8, value: u16) -> bool {
    let Ok(plan) = CodegenAbsolutePacketBuilder::new()
        .word(value)
        .head(head)
        .prepare()
    else {
        return false;
    };
    plan.commit_into(output).is_ok()
}

/// Equivalent handwritten fixed-absolute prepared-layout commit probe.
#[inline(never)]
pub fn handwritten_absolute_prepared_builder(output: &mut [u8], head: u8, value: u16) -> bool {
    if output.len() < 5 {
        false
    } else {
        output[0] = head;
        output[3..5].copy_from_slice(&value.to_be_bytes());
        true
    }
}

/// Generated declared-scalar getter probe.
#[inline(never)]
pub fn generated_scalar_getter(bytes: &[u8]) -> Option<u16> {
    CodegenScalarPacket::view(bytes)
        .without_trailing()
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
    CodegenMappedPacket::view(bytes)
        .without_trailing()
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

/// Generated relative range getter probe.
#[inline(never)]
pub fn generated_relative_range_getter(bytes: &[u8]) -> Option<u8> {
    CodegenRelativeRange::view(bytes)
        .without_trailing()
        .ok()?
        .payload()
        .first()
        .copied()
}

/// Equivalent handwritten relative range getter probe.
#[inline(never)]
pub fn handwritten_relative_range_getter(bytes: &[u8]) -> Option<u8> {
    let (&length, payload) = bytes.split_first()?;
    if payload.len() != usize::from(length) {
        return None;
    }
    payload.first().copied()
}
/// Generated relative range mutation probe.
#[inline(never)]
pub fn generated_relative_range_mutation(bytes: &mut [u8], value: u8) -> bool {
    match CodegenRelativeRangeViewMut::parse_exact_mut(bytes) {
        Ok(mut view) => match view.payload_mut().first_mut() {
            Some(byte) => {
                *byte = value;
                true
            }
            None => false,
        },
        Err(_) => false,
    }
}

/// Equivalent handwritten relative range mutation probe.
#[inline(never)]
pub fn handwritten_relative_range_mutation(bytes: &mut [u8], value: u8) -> bool {
    let Some((length, payload)) = bytes.split_first_mut() else {
        return false;
    };
    let length = *length;
    if payload.len() != usize::from(length) {
        return false;
    }
    let Some(byte) = payload.first_mut() else {
        return false;
    };
    *byte = value;
    true
}

/// Generated absolute range getter probe.
#[inline(never)]
pub fn generated_absolute_range_getter(bytes: &[u8]) -> Option<u8> {
    CodegenAbsoluteRange::view(bytes)
        .without_trailing()
        .ok()?
        .payload()
        .first()
        .copied()
}

/// Equivalent handwritten absolute range getter probe.
#[inline(never)]
pub fn handwritten_absolute_range_getter(bytes: &[u8]) -> Option<u8> {
    let (&end, remaining) = bytes.split_first()?;
    let start = bytes.len() - remaining.len();
    let end = usize::from(end);
    if end < start {
        return None;
    }
    let expected = end - start;
    if remaining.len() < expected {
        return None;
    }
    let (_, suffix) = remaining.split_at(expected);
    if !suffix.is_empty() {
        return None;
    }
    remaining.first().copied()
}

/// Generated absolute range mutation probe.
#[inline(never)]
pub fn generated_absolute_range_mutation(bytes: &mut [u8], value: u8) -> bool {
    match CodegenAbsoluteRangeViewMut::parse_exact_mut(bytes) {
        Ok(mut view) => match view.payload_mut().first_mut() {
            Some(byte) => {
                *byte = value;
                true
            }
            None => false,
        },
        Err(_) => false,
    }
}

/// Equivalent handwritten absolute range mutation probe.
#[inline(never)]
pub fn handwritten_absolute_range_mutation(bytes: &mut [u8], value: u8) -> bool {
    let Some((end, payload)) = bytes.split_first_mut() else {
        return false;
    };
    let end = *end;
    if payload.len().checked_add(1) != Some(usize::from(end)) {
        return false;
    }
    let Some(byte) = payload.first_mut() else {
        return false;
    };
    *byte = value;
    true
}

/// Generated absolute range builder probe, including derived-source failures.
#[inline(never)]
pub fn generated_absolute_range_builder(output: &mut [u8], payload: &[u8]) -> bool {
    CodegenAbsoluteRangeBuilder::new()
        .payload(payload)
        .build_into(output)
        .is_ok()
}

/// Equivalent handwritten absolute range builder probe.
#[inline(never)]
pub fn handwritten_absolute_range_builder(output: &mut [u8], payload: &[u8]) -> bool {
    let Some(end) = payload.len().checked_add(1) else {
        return false;
    };
    let Ok(end) = u8::try_from(end) else {
        return false;
    };
    if output.len() < usize::from(end) {
        return false;
    }
    output[0] = end;
    output[1..usize::from(end)].copy_from_slice(payload);
    true
}

/// Generated pre-write-derived range builder probe.
#[inline(never)]
pub fn generated_derived_range_builder(output: &mut [u8], tag: u8, payload: &[u8]) -> bool {
    CodegenDerivedRangeBuilder::new()
        .tag(tag)
        .payload(payload)
        .build_into(output)
        .is_ok()
}

/// Equivalent handwritten pre-write-derived range builder probe.
#[inline(never)]
pub fn handwritten_derived_range_builder(output: &mut [u8], tag: u8, payload: &[u8]) -> bool {
    let Ok(length) = u8::try_from(payload.len()) else {
        return false;
    };
    let Some(expected) = payload.len().checked_add(3) else {
        return false;
    };
    if output.len() < expected {
        return false;
    }
    output[0] = tag;
    output[1] = length;
    output[2..2 + payload.len()].copy_from_slice(payload);
    output[2 + payload.len()] = length;
    true
}

/// Generated post-write-finalizer builder probe.
#[inline(never)]
pub fn generated_finalizer_builder(output: &mut [u8], tag: u8) -> bool {
    CodegenFinalizedPacketBuilder::new()
        .tag(tag)
        .build_into(output)
        .is_ok()
}

/// Equivalent handwritten post-write-finalizer builder probe.
#[inline(never)]
pub fn handwritten_finalizer_builder(output: &mut [u8], tag: u8) -> bool {
    if output.len() < 3 {
        return false;
    }
    let bytes = &mut output[..3];
    bytes[0] = tag;
    bytes[1..3].fill(0);
    let checksum = codegen_finalized_sum(bytes);
    bytes[1..3].copy_from_slice(&checksum.to_be_bytes());
    true
}

/// Generated terminal range getter probe.
#[inline(never)]
pub fn generated_terminal_range_getter(bytes: &[u8]) -> Option<u8> {
    CodegenTerminalRange::view(bytes)
        .without_trailing()
        .ok()?
        .payload()
        .first()
        .copied()
}

/// Equivalent handwritten terminal range getter probe.
#[inline(never)]
pub fn handwritten_terminal_range_getter(bytes: &[u8]) -> Option<u8> {
    let (_, payload) = bytes.split_first()?;
    payload.first().copied()
}

/// Generated terminal range builder probe, including short-output behavior.
#[inline(never)]
pub fn generated_terminal_range_builder(output: &mut [u8], header: u8, payload: &[u8]) -> bool {
    CodegenTerminalRangeBuilder::new()
        .header(header)
        .payload(payload)
        .build_into(output)
        .is_ok()
}

/// Equivalent handwritten terminal range builder probe.
#[inline(never)]
pub fn handwritten_terminal_range_builder(output: &mut [u8], header: u8, payload: &[u8]) -> bool {
    let Some(expected) = 1usize.checked_add(payload.len()) else {
        return false;
    };
    if output.len() < expected {
        return false;
    }
    output[0] = header;
    output[1..expected].copy_from_slice(payload);
    true
}

/// Generated terminal existing-range builder probe, including short-output behavior.
#[inline(never)]
pub fn generated_terminal_existing_range_builder(
    output: &mut [u8],
    header: u8,
    payload_length: usize,
) -> bool {
    CodegenTerminalRangeBuilder::new()
        .header(header)
        .payload_existing(payload_length)
        .build_into(output)
        .is_ok()
}

/// Equivalent handwritten terminal existing-range builder probe.
#[inline(never)]
pub fn handwritten_terminal_existing_range_builder(
    output: &mut [u8],
    header: u8,
    payload_length: usize,
) -> bool {
    let Some(expected) = 1usize.checked_add(payload_length) else {
        return false;
    };
    if output.len() < expected {
        return false;
    }
    output[0] = header;
    true
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

    let mut generated_prepared_output = [0; 3];
    let mut handwritten_prepared_output = [0; 3];
    assert_eq!(
        black_box(generated_prepared_builder(
            black_box(&mut generated_prepared_output),
            black_box(0x1234)
        )),
        black_box(handwritten_prepared_builder(
            black_box(&mut handwritten_prepared_output),
            black_box(0x1234)
        ))
    );
    assert_eq!(generated_prepared_output, handwritten_prepared_output);

    let dynamic_payload = [0x10, 0x20];
    let mut generated_dynamic_prepared = [0xa5; 9];
    let mut handwritten_dynamic_prepared = [0xa5; 9];
    assert_eq!(
        black_box(generated_dynamic_prepared_builder(
            black_box(&mut generated_dynamic_prepared),
            black_box(0x44),
            black_box(&dynamic_payload),
        )),
        black_box(handwritten_dynamic_prepared_builder(
            black_box(&mut handwritten_dynamic_prepared),
            black_box(0x44),
            black_box(&dynamic_payload),
        ))
    );
    assert_eq!(generated_dynamic_prepared, handwritten_dynamic_prepared);
    assert_eq!(
        generated_dynamic_prepared,
        [0x44, 2, 0x10, 0x20, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5]
    );

    let mut generated_dynamic_prepared_short = [0xa5; 7];
    let mut handwritten_dynamic_prepared_short = [0xa5; 7];
    assert_eq!(
        black_box(generated_dynamic_prepared_builder(
            black_box(&mut generated_dynamic_prepared_short),
            black_box(0x44),
            black_box(&dynamic_payload),
        )),
        black_box(handwritten_dynamic_prepared_builder(
            black_box(&mut handwritten_dynamic_prepared_short),
            black_box(0x44),
            black_box(&dynamic_payload),
        ))
    );
    assert_eq!(
        generated_dynamic_prepared_short,
        handwritten_dynamic_prepared_short
    );
    assert_eq!(generated_dynamic_prepared_short, [0xa5; 7]);

    let transformed_payload = [0x10, 0x20, 0x30, 0x40];
    let transformed_input = [1, 0x10, 0x20, 0x30, 0x40];
    assert_eq!(
        black_box(generated_transformed_range_getter(black_box(
            &transformed_input
        ))),
        black_box(handwritten_transformed_range_getter(black_box(
            &transformed_input
        )))
    );
    for invalid in [
        &[][..],
        &[1, 0x10, 0x20, 0x30][..],
        &[2, 0x10, 0x20, 0x30, 0x40][..],
    ] {
        assert_eq!(
            generated_transformed_range_getter(invalid),
            handwritten_transformed_range_getter(invalid)
        );
    }

    let mut generated_transformed_prepared = [0xa5; 6];
    let mut handwritten_transformed_prepared = [0xa5; 6];
    assert_eq!(
        black_box(generated_transformed_prepared_builder(
            black_box(&mut generated_transformed_prepared),
            black_box(&transformed_payload),
        )),
        black_box(handwritten_transformed_prepared_builder(
            black_box(&mut handwritten_transformed_prepared),
            black_box(&transformed_payload),
        ))
    );
    assert_eq!(
        generated_transformed_prepared,
        handwritten_transformed_prepared
    );
    assert_eq!(
        generated_transformed_prepared,
        [1, 0x10, 0x20, 0x30, 0x40, 0xa5]
    );

    let mut generated_transformed_short = [0xa5; 4];
    let mut handwritten_transformed_short = [0xa5; 4];
    assert_eq!(
        black_box(generated_transformed_prepared_builder(
            black_box(&mut generated_transformed_short),
            black_box(&transformed_payload),
        )),
        black_box(handwritten_transformed_prepared_builder(
            black_box(&mut handwritten_transformed_short),
            black_box(&transformed_payload),
        ))
    );
    assert_eq!(generated_transformed_short, handwritten_transformed_short);
    assert_eq!(generated_transformed_short, [0xa5; 4]);

    let mut generated_transformed_unaligned = [0xa5; 6];
    let mut handwritten_transformed_unaligned = [0xa5; 6];
    assert_eq!(
        generated_transformed_prepared_builder(&mut generated_transformed_unaligned, &[0; 3]),
        handwritten_transformed_prepared_builder(&mut handwritten_transformed_unaligned, &[0; 3])
    );
    assert_eq!(
        generated_transformed_unaligned,
        handwritten_transformed_unaligned
    );
    assert_eq!(generated_transformed_unaligned, [0xa5; 6]);

    let mut generated_absolute_prepared_output = [0xa5; 6];
    let mut handwritten_absolute_prepared_output = [0xa5; 6];
    assert_eq!(
        black_box(generated_absolute_prepared_builder(
            black_box(&mut generated_absolute_prepared_output),
            black_box(0x12),
            black_box(0x3456)
        )),
        black_box(handwritten_absolute_prepared_builder(
            black_box(&mut handwritten_absolute_prepared_output),
            black_box(0x12),
            black_box(0x3456)
        ))
    );
    assert_eq!(
        generated_absolute_prepared_output,
        handwritten_absolute_prepared_output
    );
    assert_eq!(
        generated_absolute_prepared_output,
        [0x12, 0xa5, 0xa5, 0x34, 0x56, 0xa5]
    );

    let mut generated_absolute_prepared_short = [0xa5; 4];
    let mut handwritten_absolute_prepared_short = [0xa5; 4];
    assert_eq!(
        black_box(generated_absolute_prepared_builder(
            black_box(&mut generated_absolute_prepared_short),
            black_box(0x12),
            black_box(0x3456)
        )),
        black_box(handwritten_absolute_prepared_builder(
            black_box(&mut handwritten_absolute_prepared_short),
            black_box(0x12),
            black_box(0x3456)
        ))
    );
    assert_eq!(
        generated_absolute_prepared_short,
        handwritten_absolute_prepared_short
    );
    assert_eq!(generated_absolute_prepared_short, [0xa5; 4]);

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

    let mut generated_prepared_short = [0x55];
    let mut handwritten_prepared_short = [0x55];
    assert_eq!(
        black_box(generated_prepared_builder(
            black_box(&mut generated_prepared_short),
            black_box(0x1234)
        )),
        black_box(handwritten_prepared_builder(
            black_box(&mut handwritten_prepared_short),
            black_box(0x1234)
        ))
    );
    assert_eq!(generated_prepared_short, handwritten_prepared_short);

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

    let derived_payload = [0x10, 0x20];
    let mut generated_derived = [0xa5; 6];
    let mut handwritten_derived = [0xa5; 6];
    assert_eq!(
        generated_derived_range_builder(&mut generated_derived, 0xa1, &derived_payload),
        handwritten_derived_range_builder(&mut handwritten_derived, 0xa1, &derived_payload)
    );
    assert_eq!(generated_derived, handwritten_derived);

    let mut generated_finalized = [0xa5; 4];
    let mut handwritten_finalized = [0xa5; 4];
    assert_eq!(
        generated_finalizer_builder(&mut generated_finalized, 0x44),
        handwritten_finalizer_builder(&mut handwritten_finalized, 0x44)
    );
    assert_eq!(generated_finalized, handwritten_finalized);
    assert_eq!(generated_finalized, [0x44, 0x00, 0x44, 0xa5]);

    let mut generated_finalized_short = [0xa5; 2];
    let mut handwritten_finalized_short = [0xa5; 2];
    assert_eq!(
        generated_finalizer_builder(&mut generated_finalized_short, 0x44),
        handwritten_finalizer_builder(&mut handwritten_finalized_short, 0x44)
    );
    assert_eq!(generated_finalized_short, handwritten_finalized_short);
    assert_eq!(generated_finalized_short, [0xa5; 2]);

    for input in [
        &[][..],
        &[0xa1][..],
        &[0xa1, 0x10][..],
        &[0xa1, 0x10, 0x20][..],
    ] {
        assert_eq!(
            generated_terminal_range_getter(input),
            handwritten_terminal_range_getter(input)
        );
    }

    let payload = [0x10, 0x20, 0x30];
    let mut generated_terminal_range_output = [0xa5; 6];
    let mut handwritten_terminal_range_output = [0xa5; 6];
    assert_eq!(
        generated_terminal_range_builder(&mut generated_terminal_range_output, 0xa1, &payload),
        handwritten_terminal_range_builder(&mut handwritten_terminal_range_output, 0xa1, &payload)
    );
    assert_eq!(
        generated_terminal_range_output,
        handwritten_terminal_range_output
    );
    assert_eq!(
        generated_terminal_range_output,
        [0xa1, 0x10, 0x20, 0x30, 0xa5, 0xa5]
    );

    let mut generated_terminal_range_short = [0xa5; 3];
    let mut handwritten_terminal_range_short = [0xa5; 3];
    assert_eq!(
        generated_terminal_range_builder(&mut generated_terminal_range_short, 0xa1, &payload),
        handwritten_terminal_range_builder(&mut handwritten_terminal_range_short, 0xa1, &payload)
    );
    assert_eq!(
        generated_terminal_range_short,
        handwritten_terminal_range_short
    );
    assert_eq!(generated_terminal_range_short, [0xa5; 3]);

    let mut generated_existing_terminal = [0xa5; 5];
    let mut handwritten_existing_terminal = [0xa5; 5];
    assert_eq!(
        generated_terminal_existing_range_builder(&mut generated_existing_terminal, 0xa1, 2),
        handwritten_terminal_existing_range_builder(&mut handwritten_existing_terminal, 0xa1, 2)
    );
    assert_eq!(generated_existing_terminal, handwritten_existing_terminal);
    assert_eq!(generated_existing_terminal, [0xa1, 0xa5, 0xa5, 0xa5, 0xa5]);
}

#[test]
fn generated_byte_range_probes_match_handwritten_safe_rust() {
    let relative = [2, 0x10, 0x20];

    assert_eq!(
        black_box(generated_relative_range_getter(black_box(&relative))),
        black_box(handwritten_relative_range_getter(black_box(&relative)))
    );
    for invalid in [&[][..], &[0][..], &[2, 0x10][..], &[1, 0x10, 0x20][..]] {
        assert_eq!(
            generated_relative_range_getter(invalid),
            handwritten_relative_range_getter(invalid)
        );
    }

    let mut generated_relative = relative;
    let mut handwritten_relative = relative;
    assert_eq!(
        generated_relative_range_mutation(&mut generated_relative, 0xab),
        handwritten_relative_range_mutation(&mut handwritten_relative, 0xab)
    );
    assert_eq!(generated_relative, handwritten_relative);
    let mut generated_relative_invalid = [2, 0x10];
    let mut handwritten_relative_invalid = generated_relative_invalid;
    assert_eq!(
        generated_relative_range_mutation(&mut generated_relative_invalid, 0xab),
        handwritten_relative_range_mutation(&mut handwritten_relative_invalid, 0xab)
    );
    assert_eq!(generated_relative_invalid, handwritten_relative_invalid);

    let absolute = [3, 0x10, 0x20];
    assert_eq!(
        black_box(generated_absolute_range_getter(black_box(&absolute))),
        black_box(handwritten_absolute_range_getter(black_box(&absolute)))
    );
    for invalid in [&[][..], &[0][..], &[3, 0x10][..], &[2, 0x10, 0x20][..]] {
        assert_eq!(
            generated_absolute_range_getter(invalid),
            handwritten_absolute_range_getter(invalid)
        );
    }
    let mut generated_absolute = absolute;
    let mut handwritten_absolute = absolute;
    assert_eq!(
        generated_absolute_range_mutation(&mut generated_absolute, 0xab),
        handwritten_absolute_range_mutation(&mut handwritten_absolute, 0xab)
    );
    assert_eq!(generated_absolute, handwritten_absolute);
    let mut generated_absolute_invalid = [3, 0x10];
    let mut handwritten_absolute_invalid = generated_absolute_invalid;
    assert_eq!(
        generated_absolute_range_mutation(&mut generated_absolute_invalid, 0xab),
        handwritten_absolute_range_mutation(&mut handwritten_absolute_invalid, 0xab)
    );
    assert_eq!(generated_absolute_invalid, handwritten_absolute_invalid);

    let payload = [0x10, 0x20];
    let mut generated_output = [0xa5; 4];
    let mut handwritten_output = [0xa5; 4];
    assert_eq!(
        generated_absolute_range_builder(&mut generated_output, &payload),
        handwritten_absolute_range_builder(&mut handwritten_output, &payload)
    );
    assert_eq!(generated_output, handwritten_output);
    assert_eq!(generated_output, [3, 0x10, 0x20, 0xa5]);

    let mut generated_short = [0xa5; 2];
    let mut handwritten_short = [0xa5; 2];
    assert_eq!(
        generated_absolute_range_builder(&mut generated_short, &payload),
        handwritten_absolute_range_builder(&mut handwritten_short, &payload)
    );
    assert_eq!(generated_short, handwritten_short);
    assert_eq!(generated_short, [0xa5; 2]);

    let oversized = [0; 255];
    let mut generated_narrow = [0xa5; 256];
    let mut handwritten_narrow = [0xa5; 256];
    assert_eq!(
        generated_absolute_range_builder(&mut generated_narrow, &oversized),
        handwritten_absolute_range_builder(&mut handwritten_narrow, &oversized)
    );
    assert_eq!(generated_narrow, handwritten_narrow);
    assert_eq!(generated_narrow, [0xa5; 256]);
}
