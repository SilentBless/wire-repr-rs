#[path = "fixture.rs"]
mod fixture;

use fixture::CAPACITY;

pub fn decode(seed: u64) -> u64 {
    let input = fixture::input(seed);
    decode_input(input.as_slice()).unwrap_or(u64::MAX)
}

#[inline(always)]
fn decode_input(input: &[u8]) -> Option<u64> {
    let mut cursor = 0;
    let first = part(input, &mut cursor)?;
    let middle = u16::from_be_bytes(input.get(cursor..cursor.checked_add(2)?)?.try_into().ok()?);
    cursor += 2;
    let second = part(input, &mut cursor)?;
    let tail = u32::from_le_bytes(input.get(cursor..cursor.checked_add(4)?)?.try_into().ok()?);
    cursor += 4;
    (cursor == input.len()).then(|| fixture::combine(first, middle, second, tail))
}

fn part<'a>(input: &'a [u8], cursor: &mut usize) -> Option<&'a [u8]> {
    let len = usize::from(*input.get(*cursor)?);
    *cursor += 1;
    let end = cursor.checked_add(len)?;
    let payload = input.get(*cursor..end)?;
    *cursor = end;
    Some(payload)
}

pub fn build(seed: u64) -> u64 {
    fixture::hash(fixture::encoded(seed).as_slice())
}

pub fn detached_build(seed: u64) -> u64 {
    fixture::hash(core::hint::black_box(fixture::encoded(seed).as_slice()))
}

pub fn copy(seed: u64) -> u64 {
    try_copy(seed).unwrap_or(u64::MAX)
}

fn try_copy(seed: u64) -> Result<u64, ()> {
    let input = fixture::input(seed);
    decode_input(input.as_slice()).ok_or(())?;
    let mut output = [0u8; CAPACITY];
    output[..input.len].copy_from_slice(input.as_slice());
    Ok(fixture::hash(&output[..input.len]))
}

pub fn domain(seed: u64) -> u64 {
    let input = fixture::domain_input(seed);
    decode_input(input.as_slice()).unwrap_or(u64::MAX)
}
