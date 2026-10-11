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
    // Validated exact forwarding uses the same optimal bulk-copy algorithm as the
    // ordinary role; the scalar validation path below is independently optimized.
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
    let rest = input.get(cursor..)?;
    if rest.len().checked_sub(2)? != length {
        return None;
    }
    let (payload, tail) = rest.split_at(length);
    let tail = u16::from_le_bytes(tail.try_into().ok()?);
    Some((values, payload, tail))
}

// Short encodings bypass the bounded loop. Longer encodings traverse one proven
// input subslice rather than performing a new indexed bounds check for every byte.
fn read<const BITS: u32>(input: &[u8], cursor: &mut usize) -> Option<u128> {
    let (&first, rest) = input.get(*cursor..)?.split_first()?;
    if first < 0x80 {
        *cursor += 1;
        return Some(u128::from(first));
    }
    let (&second, rest) = rest.split_first()?;
    if BITS == 8 && second > 1 {
        return None;
    }
    let mut value = u128::from(first & 0x7f) | (u128::from(second & 0x7f) << 7);
    if second < 0x80 {
        *cursor += 2;
        return Some(value);
    }
    let maximum = BITS.div_ceil(7) as usize;
    for (index, &byte) in rest.iter().take(maximum - 2).enumerate() {
        let shift = (index as u32 + 2) * 7;
        let digit = byte & 0x7f;
        let remaining_bits = BITS - shift;
        if remaining_bits < 7 && u32::from(digit) >= 1 << remaining_bits {
            return None;
        }
        value |= u128::from(digit) << shift;
        if byte < 0x80 {
            *cursor += index + 3;
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
    // A raw payload's length is already known, so write its shortest prefix once.
    append(output, &mut cursor, length as u128)?;
    let body = output.get_mut(cursor..cursor + length + 2)?;
    body[..length].copy_from_slice(&payload[..length]);
    body[length..].copy_from_slice(&fixture::tail(seed).to_le_bytes());
    Some(cursor + body.len())
}

// Count the groups once and prove the whole destination span before filling it.
// There is no per-group termination branch or indexed destination bounds check.
fn append(output: &mut [u8], cursor: &mut usize, value: u128) -> Option<()> {
    let groups = (128 - value.leading_zeros()).max(1).div_ceil(7) as usize;
    let end = cursor.checked_add(groups)?;
    let bytes = output.get_mut(*cursor..end)?;
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = (((value >> (index * 7)) & 0x7f) as u8) | 0x80;
    }
    *bytes.last_mut()? &= 0x7f;
    *cursor = end;
    Some(())
}
