#![allow(dead_code)]

#[path = "../fixture.rs"]
mod fixture;

use fixture::{CAPACITY, Values};

pub fn decode(seed: u64) -> u64 {
    let (input, length) = fixture::source(seed);
    let Some((values, payload, tail)) = frame(&input[..length]) else {
        return u64::MAX;
    };
    fixture::observe(values, payload, tail)
}

pub fn build(seed: u64) -> u64 {
    let mut output = [0u8; CAPACITY];
    let Some(length) = write(seed, &mut output) else {
        return u64::MAX;
    };
    fixture::hash(&output[..length])
}

pub fn copy(seed: u64) -> u64 {
    let (input, length) = fixture::source(seed);
    let source = &input[..length];
    if frame(source).is_none() {
        return u64::MAX;
    }
    let mut output = [0u8; CAPACITY + 1];
    output[0] = 1;
    output[1..length + 1].copy_from_slice(source);
    fixture::hash(&output[..length + 1])
}

fn frame(input: &[u8]) -> Option<(Values, &[u8], u16)> {
    let mut cursor = 0;
    let signed = read_signed(input, &mut cursor)?;
    let transformed = read_unsigned(input, &mut cursor)?;
    let prefix = u64::try_from(read_prefix(input, &mut cursor)?).ok()?;
    let signed_prefix = read_prefix(input, &mut cursor)?;
    let length = u16::try_from(read_prefix(input, &mut cursor)?).ok()? as usize;
    let rest = input.get(cursor..)?;
    if rest.len().checked_sub(2)? != length {
        return None;
    }
    let (payload, tail) = rest.split_at(length);
    let tail = u16::from_le_bytes(tail.try_into().ok()?);
    let transformed = if transformed & 1 == 0 {
        (transformed >> 1) as i32
    } else {
        -((transformed >> 1) as i32) - 1
    };
    let signed_prefix = if signed_prefix & 1 == 0 {
        (signed_prefix >> 1) as i128
    } else {
        -((signed_prefix >> 1) as i128) - 1
    };
    Some((
        Values {
            signed,
            transformed,
            prefix,
            signed_prefix,
        },
        payload,
        tail,
    ))
}

// Find the terminator in the bounded source first, then decode one proven span.
// The common single-byte path bypasses both traversals.
fn read_signed(input: &[u8], cursor: &mut usize) -> Option<i64> {
    let source = input.get(*cursor..)?;
    let first = *source.first()?;
    if first < 0x80 {
        *cursor += 1;
        return Some(i64::from((first << 1) as i8 >> 1));
    }
    let length = source.iter().take(10).position(|byte| *byte < 0x80)? + 1;
    let bytes = &source[..length];
    let terminal = *bytes.last()? & 0x7f;
    if length == 10 && terminal != 0 && terminal != 0x7f {
        return None;
    }
    let mut bits = 0u128;
    for (group, &byte) in bytes.iter().enumerate() {
        bits |= u128::from(byte & 0x7f) << (group * 7);
    }
    let extension = 128 - length * 7;
    let value = ((bits << extension) as i128) >> extension;
    let value = i64::try_from(value).ok()?;
    *cursor += length;
    Some(value)
}

fn read_unsigned(input: &[u8], cursor: &mut usize) -> Option<u32> {
    let source = input.get(*cursor..)?;
    let first = *source.first()?;
    if first < 0x80 {
        *cursor += 1;
        return Some(u32::from(first));
    }
    let length = source.iter().take(5).position(|byte| *byte < 0x80)? + 1;
    let bytes = &source[..length];
    if length == 5 && *bytes.last()? > 0x0f {
        return None;
    }
    let mut value = 0u32;
    for (group, &byte) in bytes.iter().enumerate() {
        value |= u32::from(byte & 0x7f) << (group * 7);
    }
    *cursor += length;
    Some(value)
}

// Prefix geometry proves the complete source once. Binary payloads use a safe
// little-endian load; unary payloads require only a small fixed-width load.
fn read_prefix(input: &[u8], cursor: &mut usize) -> Option<u128> {
    let source = input.get(*cursor..)?;
    let first = *source.first()?;
    if first < 0x80 {
        *cursor += 1;
        return Some(u128::from(first));
    }
    let unary = first.leading_ones() as usize;
    let length = if unary < 4 {
        unary + 1
    } else {
        usize::from(first & 15) + 2
    };
    let bytes = source.get(..length)?;
    let value = if unary < 4 {
        let mut payload = [0u8; 4];
        payload[..length - 1].copy_from_slice(&bytes[1..]);
        u128::from(first & (0x7f >> unary))
            | (u128::from(u32::from_le_bytes(payload)) << (7 - unary))
    } else {
        let mut payload = [0u8; 16];
        payload[..length - 1].copy_from_slice(&bytes[1..]);
        u128::from_le_bytes(payload)
    };
    *cursor += length;
    Some(value)
}

fn write(seed: u64, output: &mut [u8; CAPACITY]) -> Option<usize> {
    let values = fixture::values(seed);
    let (payload, length) = fixture::payload(seed);
    let mut cursor = 0;
    append_signed(output, &mut cursor, values.signed)?;
    append_unsigned(
        output,
        &mut cursor,
        ((values.transformed as u32) << 1) ^ ((values.transformed >> 31) as u32),
    )?;
    append_prefix(output, &mut cursor, u128::from(values.prefix))?;
    append_prefix(
        output,
        &mut cursor,
        ((values.signed_prefix as u128) << 1) ^ ((values.signed_prefix >> 127) as u128),
    )?;
    append_prefix(output, &mut cursor, length as u128)?;
    let end = cursor.checked_add(length)?.checked_add(2)?;
    let body = output.get_mut(cursor..end)?;
    body[..length].copy_from_slice(&payload[..length]);
    body[length..].copy_from_slice(&fixture::tail(seed).to_le_bytes());
    Some(end)
}

fn append_signed(output: &mut [u8], cursor: &mut usize, value: i64) -> Option<()> {
    let magnitude = (value ^ (value >> 63)) as u64;
    let groups = (65 - magnitude.leading_zeros()).div_ceil(7) as usize;
    let end = cursor.checked_add(groups)?;
    let bytes = output.get_mut(*cursor..end)?;
    for (group, byte) in bytes.iter_mut().enumerate() {
        *byte = (((value >> (group * 7)) & 0x7f) as u8) | 0x80;
    }
    *bytes.last_mut()? &= 0x7f;
    *cursor = end;
    Some(())
}

fn append_unsigned(output: &mut [u8], cursor: &mut usize, value: u32) -> Option<()> {
    let groups = (32 - value.leading_zeros()).max(1).div_ceil(7) as usize;
    let end = cursor.checked_add(groups)?;
    let bytes = output.get_mut(*cursor..end)?;
    for (group, byte) in bytes.iter_mut().enumerate() {
        *byte = (((value >> (group * 7)) & 0x7f) as u8) | 0x80;
    }
    *bytes.last_mut()? &= 0x7f;
    *cursor = end;
    Some(())
}

fn append_prefix(output: &mut [u8], cursor: &mut usize, value: u128) -> Option<()> {
    let bits = 128 - value.leading_zeros();
    let length = match bits {
        0..=7 => 1,
        8..=14 => 2,
        15..=21 => 3,
        22..=28 => 4,
        _ => bits.div_ceil(8) as usize + 1,
    };
    let end = cursor.checked_add(length)?;
    let bytes = output.get_mut(*cursor..end)?;
    match length {
        1 => bytes[0] = value as u8,
        2..=4 => {
            let first_bits = 8 - length;
            bytes[0] = (0xff << (9 - length)) | (value & ((1 << first_bits) - 1)) as u8;
            let payload = (value >> first_bits).to_le_bytes();
            bytes[1..].copy_from_slice(&payload[..length - 1]);
        }
        _ => {
            bytes[0] = 0xf0 | (length as u8 - 2);
            bytes[1..].copy_from_slice(&value.to_le_bytes()[..length - 1]);
        }
    }
    *cursor = end;
    Some(())
}
