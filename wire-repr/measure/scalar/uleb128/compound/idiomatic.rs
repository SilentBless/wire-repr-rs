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
    let values = Values {
        byte: read::<8>(input, &mut cursor)? as u8,
        short: read::<16>(input, &mut cursor)? as u16,
        word: read::<32>(input, &mut cursor)? as u32,
        long: read::<64>(input, &mut cursor)? as u64,
        wide: read::<128>(input, &mut cursor)?,
    };
    let length = read::<16>(input, &mut cursor)? as usize;
    let payload_end = cursor.checked_add(length)?;
    let payload = input.get(cursor..payload_end)?;
    let end = payload_end.checked_add(2)?;
    let tail = u16::from_le_bytes(input.get(payload_end..end)?.try_into().ok()?);
    if end != input.len() {
        return None;
    }
    Some((values, payload, tail))
}

fn read<const BITS: u32>(input: &[u8], cursor: &mut usize) -> Option<u128> {
    let mut value = 0u128;
    for group in 0..BITS.div_ceil(7) {
        let byte = *input.get(*cursor)?;
        *cursor += 1;
        let digit = byte & 0x7f;
        let remaining_bits = BITS - group * 7;
        if remaining_bits < 7 && u32::from(digit) >= 1 << remaining_bits {
            return None;
        }
        value |= u128::from(digit) << (group * 7);
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn write(seed: u64, output: &mut [u8; CAPACITY]) -> Option<usize> {
    let values = fixture::values(seed);
    let (payload, length) = fixture::payload(seed);
    let mut cursor = 0;
    append(output, &mut cursor, u128::from(values.byte))?;
    append(output, &mut cursor, u128::from(values.short))?;
    append(output, &mut cursor, u128::from(values.word))?;
    append(output, &mut cursor, u128::from(values.long))?;
    append(output, &mut cursor, values.wide)?;
    append(output, &mut cursor, length as u128)?;
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

fn append(output: &mut [u8], cursor: &mut usize, mut value: u128) -> Option<()> {
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
