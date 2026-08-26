#![allow(dead_code)]

pub fn decode(seed: u64) -> u64 {
    let input = opaque_input(seed);
    try_decode(&input).unwrap_or(u64::MAX)
}

fn try_decode(input: &[u8]) -> Option<u64> {
    let tail = validate(input)?;
    let count = usize::from(*input.first()?);
    let mut cursor = 1usize;
    let mut value = u64::from(*input.get(tail)?).rotate_left(53);
    for _ in 0..count {
        let len = usize::from(*input.get(cursor)?);
        cursor += 1;
        let body = input.get(cursor..cursor.checked_add(len)?)?;
        for byte in body {
            value = value.rotate_left(7) ^ u64::from(*byte);
        }
        cursor += len;
    }
    Some(value)
}

pub fn build(seed: u64) -> u64 {
    hash(&input(seed))
}

pub fn copy(seed: u64) -> u64 {
    let source = opaque_input(seed);
    try_copy(&source).unwrap_or(u64::MAX)
}

fn try_copy(source: &[u8]) -> Option<u64> {
    let tail = validate(source)?;
    let mut output = [0u8; 9];
    output[0] = *source.first()?;
    let count = usize::from(output[0]);
    let mut source_cursor = 1usize;
    let mut output_cursor = 1usize;
    for _ in 0..count {
        let len = usize::from(*source.get(source_cursor)?);
        let end = source_cursor.checked_add(1 + len)?;
        let item = source.get(source_cursor..end)?;
        output
            .get_mut(output_cursor..output_cursor + item.len())?
            .copy_from_slice(item);
        source_cursor = end;
        output_cursor += item.len();
    }
    output[output_cursor] = *source.get(tail)?;
    Some(hash(&output))
}

fn validate(input: &[u8]) -> Option<usize> {
    let count = usize::from(*input.first()?);
    let mut cursor = 1usize;
    for _ in 0..count {
        let len = usize::from(*input.get(cursor)?);
        cursor = cursor.checked_add(1 + len)?;
        input.get(..=cursor.saturating_sub(1))?;
    }
    (cursor + 1 == input.len()).then_some(cursor)
}

#[inline(never)]
fn opaque_input(seed: u64) -> [u8; 9] {
    input(seed)
}

fn input(seed: u64) -> [u8; 9] {
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

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
