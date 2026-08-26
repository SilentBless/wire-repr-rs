#![allow(dead_code)]

pub fn decode(seed: u64) -> u64 {
    let input = opaque_input(seed);
    decode_input(input.as_slice()).unwrap_or(u64::MAX)
}

fn decode_input(input: &[u8]) -> Option<u64> {
    let count = usize::from(*input.first()?);
    let mut cursor = 1usize;
    let mut value = u64::from(*input.last()?).rotate_left(53);
    for _ in 0..count {
        let length = usize::from(*input.get(cursor)?);
        cursor += 1;
        let body = input.get(cursor..cursor.checked_add(length)?)?;
        for byte in body {
            value = value.rotate_left(7) ^ u64::from(*byte);
        }
        cursor += length;
    }
    (cursor + 1 == input.len()).then_some(value)
}

pub fn build(seed: u64) -> u64 {
    hash(&build_input(seed))
}

pub fn copy(seed: u64) -> u64 {
    let source = opaque_input(seed);
    copy_input(source.as_slice()).unwrap_or(u64::MAX)
}

fn copy_input(source: &[u8]) -> Option<u64> {
    let tail = validate(source)?;
    let mut output = [0u8; 16];
    output.get_mut(..tail)?.copy_from_slice(source.get(..tail)?);
    output[tail] = source[tail].wrapping_add(1);
    Some(observe(&output[..=tail]))
}

pub fn domain(seed: u64) -> u64 {
    let input = domain_input(seed);
    let decode = decode_input(input.as_slice()).unwrap_or(u64::MAX);
    let copy = copy_input(input.as_slice()).unwrap_or(u64::MAX);
    decode.rotate_left(17) ^ copy
}

fn validate(input: &[u8]) -> Option<usize> {
    let count = usize::from(*input.first()?);
    let mut cursor = 1usize;
    for _ in 0..count {
        let length = usize::from(*input.get(cursor)?);
        cursor = cursor.checked_add(1 + length)?;
        input.get(..cursor)?;
    }
    (cursor + 1 == input.len()).then_some(cursor)
}

struct Input {
    bytes: [u8; 16],
    len: usize,
}

impl Input {
    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[inline(never)]
fn opaque_input(seed: u64) -> Input {
    input(seed)
}

fn input(seed: u64) -> Input {
    let mut bytes = [0u8; 16];
    bytes[0] = 3;
    let mut cursor = 1usize;
    for index in 0..3usize {
        let length = ((seed >> (index * 8)) as usize % 3) + 1;
        bytes[cursor] = length as u8;
        cursor += 1;
        for item in 0..length {
            bytes[cursor] = seed.rotate_right((index * 11 + item * 7) as u32) as u8;
            cursor += 1;
        }
    }
    bytes[cursor] = (seed >> 32) as u8;
    Input {
        bytes,
        len: cursor + 1,
    }
}

fn domain_input(seed: u64) -> Input {
    let mut input = input(seed);
    match seed & 7 {
        0 => input.bytes[0] = 0,
        1 => input.bytes[0] = 4,
        2 => input.bytes[1] = 15,
        3 => input.len -= 1,
        _ => {}
    }
    input
}

fn build_input(seed: u64) -> [u8; 9] {
    [
        3,
        1,
        seed as u8,
        2,
        (seed >> 8) as u8,
        (seed >> 16) as u8,
        1,
        (seed >> 24) as u8,
        (seed >> 32) as u8,
    ]
}

#[inline(never)]
fn observe(bytes: &[u8]) -> u64 {
    hash(bytes)
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
