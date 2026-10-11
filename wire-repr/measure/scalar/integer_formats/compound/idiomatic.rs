#![allow(dead_code)]

#[path = "fixture.rs"]
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
    let payload_end = cursor.checked_add(length)?;
    let payload = input.get(cursor..payload_end)?;
    let end = payload_end.checked_add(2)?;
    let tail = u16::from_le_bytes(input.get(payload_end..end)?.try_into().ok()?);
    if end != input.len() {
        return None;
    }
    Some((
        Values {
            signed,
            transformed: ((transformed >> 1) as i32) ^ -((transformed & 1) as i32),
            prefix,
            signed_prefix: ((signed_prefix >> 1) as i128) ^ -((signed_prefix & 1) as i128),
        },
        payload,
        tail,
    ))
}

fn read_signed(input: &[u8], cursor: &mut usize) -> Option<i64> {
    let mut value = 0u128;
    for group in 0..10 {
        let byte = *input.get(*cursor)?;
        *cursor += 1;
        let digit = byte & 0x7f;
        if group == 9 && digit != 0 && digit != 0x7f {
            return None;
        }
        value |= u128::from(digit) << (group * 7);
        if byte < 0x80 {
            if byte & 0x40 != 0 {
                value |= u128::MAX << ((group + 1) * 7);
            }
            return i64::try_from(value as i128).ok();
        }
    }
    None
}

fn read_unsigned(input: &[u8], cursor: &mut usize) -> Option<u32> {
    let mut value = 0u64;
    for group in 0..5 {
        let byte = *input.get(*cursor)?;
        *cursor += 1;
        value |= u64::from(byte & 0x7f) << (group * 7);
        if byte < 0x80 {
            return u32::try_from(value).ok();
        }
    }
    None
}

fn read_prefix(input: &[u8], cursor: &mut usize) -> Option<u128> {
    let first = *input.get(*cursor)?;
    *cursor += 1;
    let (payload, first_bits) = match first {
        0x00..=0x7f => return Some(u128::from(first)),
        0x80..=0xbf => (1, 6),
        0xc0..=0xdf => (2, 5),
        0xe0..=0xef => (3, 4),
        _ => (usize::from(first & 15) + 1, 0),
    };
    let mut value = if first_bits == 0 {
        0
    } else {
        u128::from(first & ((1 << first_bits) - 1))
    };
    for index in 0..payload {
        let byte = *input.get(*cursor)?;
        *cursor += 1;
        value |= u128::from(byte) << (first_bits + index * 8);
    }
    Some(value)
}

fn write(seed: u64, output: &mut [u8; CAPACITY]) -> Option<usize> {
    let values = fixture::values(seed);
    let (payload, length) = fixture::payload(seed);
    let mut cursor = 0;
    append_signed(output, &mut cursor, values.signed)?;
    let transformed = if values.transformed < 0 {
        values
            .transformed
            .unsigned_abs()
            .wrapping_mul(2)
            .wrapping_sub(1)
    } else {
        (values.transformed as u32) * 2
    };
    append_unsigned(output, &mut cursor, transformed)?;
    append_prefix(output, &mut cursor, u128::from(values.prefix))?;
    let signed_prefix = if values.signed_prefix < 0 {
        values
            .signed_prefix
            .unsigned_abs()
            .wrapping_mul(2)
            .wrapping_sub(1)
    } else {
        (values.signed_prefix as u128) * 2
    };
    append_prefix(output, &mut cursor, signed_prefix)?;
    append_prefix(output, &mut cursor, length as u128)?;
    let payload_end = cursor.checked_add(length)?;
    output
        .get_mut(cursor..payload_end)?
        .copy_from_slice(&payload[..length]);
    let end = payload_end.checked_add(2)?;
    output
        .get_mut(payload_end..end)?
        .copy_from_slice(&fixture::tail(seed).to_le_bytes());
    Some(end)
}

fn append_signed(output: &mut [u8], cursor: &mut usize, mut value: i64) -> Option<()> {
    loop {
        let digit = (value & 0x7f) as u8;
        value >>= 7;
        let last = (value == 0 && digit & 0x40 == 0) || (value == -1 && digit & 0x40 != 0);
        *output.get_mut(*cursor)? = digit | if last { 0 } else { 0x80 };
        *cursor += 1;
        if last {
            return Some(());
        }
    }
}

fn append_unsigned(output: &mut [u8], cursor: &mut usize, mut value: u32) -> Option<()> {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        *output.get_mut(*cursor)? = byte;
        *cursor += 1;
        if value == 0 {
            return Some(());
        }
    }
}

fn append_prefix(output: &mut [u8], cursor: &mut usize, mut value: u128) -> Option<()> {
    let (prefix, first_bits, payload) = if value < 1 << 7 {
        (0, 7, 0)
    } else if value < 1 << 14 {
        (0x80, 6, 1)
    } else if value < 1 << 21 {
        (0xc0, 5, 2)
    } else if value < 1 << 28 {
        (0xe0, 4, 3)
    } else {
        let bytes = (128 - value.leading_zeros()).div_ceil(8) as usize;
        (0xf0 | (bytes as u8 - 1), 0, bytes)
    };
    let first = if first_bits == 0 {
        prefix
    } else {
        prefix | (value & ((1 << first_bits) - 1)) as u8
    };
    *output.get_mut(*cursor)? = first;
    *cursor += 1;
    value >>= first_bits;
    for _ in 0..payload {
        *output.get_mut(*cursor)? = value as u8;
        *cursor += 1;
        value >>= 8;
    }
    Some(())
}
