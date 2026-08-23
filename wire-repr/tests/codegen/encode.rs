use super::schema::{ComputedPacket, FixedPacket, PositionedPacket};

/// Encodes one fixed representation through generated preparation and commit.
#[inline(never)]
pub(super) fn generated_fixed_encode(word: u16, output: &mut [u8]) -> usize {
    FixedPacket { word }
        .build_into(output)
        .map_or(0, |(written, _)| written.as_bytes().len())
}

/// Encodes the fixed representation directly after capacity preflight.
#[inline(never)]
pub(super) fn handwritten_fixed_encode(word: u16, output: &mut [u8]) -> usize {
    if output.len() < 2 {
        return 0;
    }
    output[..2].copy_from_slice(&word.to_be_bytes());
    2
}
/// Encodes a fixed forward position through generated derive code.
#[inline(never)]
pub(super) fn generated_positioned_encode(word: u16, output: &mut [u8]) -> usize {
    PositionedPacket { tag: 9, word }
        .build_into(output)
        .map_or(0, |(written, _)| written.as_bytes().len())
}

/// Encodes the same fixed forward position directly.
#[inline(never)]
pub(super) fn handwritten_positioned_encode(word: u16, output: &mut [u8]) -> usize {
    if output.len() < 6 {
        return 0;
    }
    output[0] = 9;
    output[1..4].fill(0);
    output[4..6].copy_from_slice(&word.to_be_bytes());
    6
}
/// Encodes a computed payload length through generated preparation and commit.
#[inline(never)]
pub(super) fn generated_computed_encode(payload: &[u8], output: &mut [u8]) -> usize {
    ComputedPacket::builder()
        .kind(9)
        .payload(payload)
        .build_into(output)
        .map_or(0, |(written, _)| written.as_bytes().len())
}

/// Encodes the computed payload length directly after preflight.
#[inline(never)]
pub(super) fn handwritten_computed_encode(payload: &[u8], output: &mut [u8]) -> usize {
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
