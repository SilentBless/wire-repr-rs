//! Release-codegen regression probes for the derive frontend.

use core::hint::black_box;
use wire_repr::Wire;

#[derive(Wire)]
struct FixedPacket {
    #[wire(be)]
    word: u16,
}

#[allow(dead_code)]
#[derive(Wire)]
struct DynamicPacket<'wire> {
    length: u8,
    #[wire(bytes = length)]
    payload: &'wire [u8],
    tail: u8,
}

#[derive(Wire)]
struct ChoiceBody {
    #[wire(be)]
    value: u16,
}

#[allow(dead_code)]
#[derive(Wire)]
#[wire(tag = U8)]
#[wire(unknown = reject)]
#[repr(u8)]
enum CodegenChoice {
    Halt = 1,
    Data(ChoiceBody) = 2,
}

#[allow(dead_code)]
#[derive(Wire)]
#[wire(tag = [u8; 4], unknown = reject)]
enum CodegenByteChoice {
    #[wire(tag = b"HALT")]
    Halt,
    #[wire(tag = b"DATA")]
    Data(ChoiceBody),
}

#[derive(Wire)]
#[wire(bitfield = u16, be, reserved = zero)]
struct CodegenFlags {
    #[wire(bit = 0)]
    enabled: bool,
    #[wire(bits = 1..=3)]
    mode: u8,
}

#[derive(Wire)]
struct PositionedPacket {
    tag: u8,
    #[wire(at = 4, be)]
    word: u16,
}

/// Decodes one fixed representation through generated derive code.
#[inline(never)]
pub fn generated_fixed_decode(bytes: &[u8]) -> u16 {
    FixedPacket::view(bytes)
        .with_remainder()
        .map_or(u16::MAX, |(packet, _)| packet.word())
}

/// Decodes the fixed representation directly.
#[inline(never)]
pub fn handwritten_fixed_decode(bytes: &[u8]) -> u16 {
    let Some(bytes) = bytes.get(..2) else {
        return u16::MAX;
    };
    u16::from_be_bytes([bytes[0], bytes[1]])
}

/// Encodes one fixed representation through generated preparation and commit.
#[inline(never)]
pub fn generated_fixed_encode(word: u16, output: &mut [u8]) -> usize {
    FixedPacket { word }
        .build_into(output)
        .map_or(0, |(written, _)| written.as_bytes().len())
}

/// Encodes the fixed representation directly after capacity preflight.
#[inline(never)]
pub fn handwritten_fixed_encode(word: u16, output: &mut [u8]) -> usize {
    if output.len() < 2 {
        return 0;
    }
    output[..2].copy_from_slice(&word.to_be_bytes());
    2
}

/// Decodes bounded borrowed bytes through generated derive code.
#[inline(never)]
pub fn generated_bounded_decode(bytes: &[u8]) -> u8 {
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

/// Decodes the same bounded bytes directly.
#[inline(never)]
pub fn handwritten_bounded_decode(bytes: &[u8]) -> u8 {
    let Some((&length, remaining)) = bytes.split_first() else {
        return u8::MAX;
    };
    let length = usize::from(length);
    if remaining.len() != length + 1 || length == 0 {
        return u8::MAX;
    }
    remaining[0] ^ remaining[length] ^ length as u8
}

/// Dispatches a tagged enum through generated derive code.
#[inline(never)]
pub fn generated_enum_decode(bytes: &[u8]) -> u16 {
    match CodegenChoice::view(bytes).without_trailing() {
        Ok(choice) if choice.is_halt() => 0,
        Ok(choice) => choice.data().map_or(u16::MAX, |body| body.value()),
        Err(_) => u16::MAX,
    }
}

/// Dispatches the same tagged representation directly.
#[inline(never)]
pub fn handwritten_enum_decode(bytes: &[u8]) -> u16 {
    match bytes {
        [1] => 0,
        [2, high, low] => u16::from_be_bytes([*high, *low]),
        _ => u16::MAX,
    }
}

/// Dispatches a fixed byte-tagged enum through generated derive code.
#[inline(never)]
pub fn generated_byte_enum_decode(bytes: &[u8]) -> u16 {
    match CodegenByteChoice::view(bytes).without_trailing() {
        Ok(choice) if choice.is_halt() => 0,
        Ok(choice) => choice.data().map_or(u16::MAX, |body| body.value()),
        Err(_) => u16::MAX,
    }
}

/// Dispatches the same fixed byte-tagged representation directly.
#[inline(never)]
pub fn handwritten_byte_enum_decode(bytes: &[u8]) -> u16 {
    match bytes {
        b"HALT" => 0,
        [b'D', b'A', b'T', b'A', high, low] => u16::from_be_bytes([*high, *low]),
        _ => u16::MAX,
    }
}

/// Decodes nominal bit projections through generated derive code.
#[inline(never)]
pub fn generated_bitfield_decode(bytes: &[u8]) -> u8 {
    CodegenFlags::view(bytes)
        .without_trailing()
        .map_or(u8::MAX, |flags| {
            u8::from(flags.enabled()) | (flags.mode() << 1)
        })
}

/// Decodes the same bit projections directly.
#[inline(never)]
pub fn handwritten_bitfield_decode(bytes: &[u8]) -> u8 {
    let [high, low] = bytes else {
        return u8::MAX;
    };
    let raw = u16::from_be_bytes([*high, *low]);
    u8::from(raw & 1 != 0) | (((raw >> 1) as u8 & 0x07) << 1)
}

/// Sums a fixed sequence through the generated infallible view iterator.
#[inline(never)]
pub fn generated_fixed_sequence(bytes: &[u8]) -> u16 {
    let Ok(packets) = FixedPacket::views(bytes) else {
        return u16::MAX;
    };
    packets.fold(0, |sum, packet| sum.wrapping_add(packet.word()))
}

/// Sums the same fixed-width sequence directly.
#[inline(never)]
pub fn handwritten_fixed_sequence(bytes: &[u8]) -> u16 {
    let chunks = bytes.chunks_exact(2);
    if !chunks.remainder().is_empty() {
        return u16::MAX;
    }
    chunks.fold(0, |sum, bytes| {
        sum.wrapping_add(u16::from_be_bytes([bytes[0], bytes[1]]))
    })
}

/// Walks variable-width records through the generated fail-closed cursor.
#[inline(never)]
pub fn generated_variable_cursor(bytes: &[u8]) -> u8 {
    let mut records = DynamicPacket::cursor(bytes);
    let mut sum = 0u8;
    loop {
        match records.next() {
            Ok(Some(record)) => {
                sum = sum
                    .wrapping_add(record.length())
                    .wrapping_add(record.tail());
            }
            Ok(None) => return sum,
            Err(_) => return u8::MAX,
        }
    }
}

/// Walks the same variable-width records directly.
#[inline(never)]
pub fn handwritten_variable_cursor(mut bytes: &[u8]) -> u8 {
    let mut sum = 0u8;
    while let Some((&length, remaining)) = bytes.split_first() {
        let length = usize::from(length);
        let tail_index = length;
        let Some((&tail, suffix)) = remaining
            .get(tail_index)
            .zip(remaining.get(tail_index + 1..))
        else {
            return u8::MAX;
        };
        sum = sum.wrapping_add(length as u8).wrapping_add(tail);
        bytes = suffix;
    }
    sum
}

/// Encodes a fixed forward position through generated derive code.
#[inline(never)]
pub fn generated_positioned_encode(word: u16, output: &mut [u8]) -> usize {
    PositionedPacket { tag: 9, word }
        .build_into(output)
        .map_or(0, |(written, _)| written.as_bytes().len())
}

/// Encodes the same fixed forward position directly.
#[inline(never)]
pub fn handwritten_positioned_encode(word: u16, output: &mut [u8]) -> usize {
    if output.len() < 6 {
        return 0;
    }
    output[0] = 9;
    output[1..4].fill(0);
    output[4..6].copy_from_slice(&word.to_be_bytes());
    6
}

#[test]
fn generated_probes_match_handwritten_safe_rust() {
    for bytes in [&[][..], &[0x12], &[0x12, 0x34], &[0x12, 0x34, 0]] {
        assert_eq!(
            generated_fixed_decode(black_box(bytes)),
            handwritten_fixed_decode(black_box(bytes))
        );
    }
    for bytes in [&[][..], &[2, 0xaa, 0xbb, 0x55], &[0, 0x55], &[1, 0xaa]] {
        assert_eq!(
            generated_bounded_decode(black_box(bytes)),
            handwritten_bounded_decode(black_box(bytes))
        );
    }
    for bytes in [&[][..], &[1], &[2, 0x12, 0x34], &[3]] {
        assert_eq!(
            generated_enum_decode(black_box(bytes)),
            handwritten_enum_decode(black_box(bytes))
        );
    }

    for bytes in [
        &[][..],
        b"HALT" as &[u8],
        b"DATA\x12\x34" as &[u8],
        b"NOPE" as &[u8],
    ] {
        assert_eq!(
            generated_byte_enum_decode(black_box(bytes)),
            handwritten_byte_enum_decode(black_box(bytes))
        );
    }

    for bytes in [&[][..], &[0x00], &[0x00, 0x0b], &[0xff, 0xff]] {
        assert_eq!(
            generated_bitfield_decode(black_box(bytes)),
            handwritten_bitfield_decode(black_box(bytes))
        );
    }
    for bytes in [
        &[][..],
        &[0x12][..],
        &[0x12, 0x34][..],
        &[0x12, 0x34, 0xab, 0xcd][..],
    ] {
        assert_eq!(
            generated_fixed_sequence(black_box(bytes)),
            handwritten_fixed_sequence(black_box(bytes))
        );
    }
    for bytes in [
        &[][..],
        &[0, 1][..],
        &[1, 9, 2][..],
        &[1, 9, 2, 0, 3][..],
        &[2, 9][..],
    ] {
        assert_eq!(
            generated_variable_cursor(black_box(bytes)),
            handwritten_variable_cursor(black_box(bytes))
        );
    }

    for length in [0, 2, 3, 6, 8] {
        let mut generated = [0xa5; 8];
        let mut handwritten = generated;
        let generated_len = generated_fixed_encode(0x1234, black_box(&mut generated[..length]));
        let handwritten_len =
            handwritten_fixed_encode(0x1234, black_box(&mut handwritten[..length]));
        assert_eq!(generated_len, handwritten_len);
        assert_eq!(generated, handwritten);

        generated.fill(0xa5);
        handwritten.fill(0xa5);
        let generated_len =
            generated_positioned_encode(0x1234, black_box(&mut generated[..length]));
        let handwritten_len =
            handwritten_positioned_encode(0x1234, black_box(&mut handwritten[..length]));
        assert_eq!(generated_len, handwritten_len);
        assert_eq!(generated, handwritten);
    }
}
