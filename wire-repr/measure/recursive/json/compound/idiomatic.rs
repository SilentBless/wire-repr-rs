#![allow(dead_code)]

const COUNT: u16 = 200;
const DEPTH: usize = 64;

pub fn direct_batch(seed: u64) -> u64 {
    let input = opaque_input(seed, true);
    let bytes = input.as_slice();
    if skip_value(bytes, 0) != Some(bytes.len()) {
        return u64::MAX;
    }
    let mut hash = seed;
    for query in 0..COUNT {
        let requested = (usize::from(query) * 37 + seed as usize) % usize::from(COUNT);
        let mut cursor = 3usize;
        for _ in 0..requested {
            cursor = match skip_value(bytes, cursor) {
                Some(cursor) => cursor,
                None => return u64::MAX,
            };
        }
        let Some(value) = observe(bytes, cursor) else {
            return u64::MAX;
        };
        hash = hash.rotate_left(7) ^ value;
    }
    hash
}

pub fn direct_lookup(seed: u64) -> u64 {
    lookup(seed, true).unwrap_or(u64::MAX)
}

pub fn replay_lookup(seed: u64) -> u64 {
    lookup(seed, false).unwrap_or(u64::MAX)
}

fn lookup(seed: u64, periodic: bool) -> Option<u64> {
    let input = opaque_input(seed, periodic);
    let bytes = input.as_slice();
    (skip_value(bytes, 0)? == bytes.len()).then_some(())?;
    let requested = seed as usize % usize::from(COUNT);
    let mut cursor = 3usize;
    for index in 0..=requested {
        let end = skip_value(bytes, cursor)?;
        if index == requested {
            return observe(bytes, cursor).filter(|_| end <= bytes.len());
        }
        cursor = end;
    }
    None
}

pub fn iterate(seed: u64) -> u64 {
    let input = opaque_input(seed, false);
    let bytes = input.as_slice();
    if skip_value(bytes, 0) != Some(bytes.len()) {
        return u64::MAX;
    }
    let mut cursor = 3usize;
    let mut hash = seed;
    for _ in 0..COUNT {
        let Some(end) = skip_value(bytes, cursor) else {
            return u64::MAX;
        };
        let Some(value) = observe(bytes, cursor) else {
            return u64::MAX;
        };
        hash = hash.rotate_left(7) ^ value;
        cursor = end;
    }
    (cursor == bytes.len()).then_some(hash).unwrap_or(u64::MAX)
}

fn observe(input: &[u8], start: usize) -> Option<u64> {
    match *input.get(start)? {
        0 => Some(0),
        1 => Some(u64::from(*input.get(start + 1)?) + 1),
        2 => Some(
            u64::from(u16::from_le_bytes(
                input.get(start + 1..start + 3)?.try_into().ok()?,
            )) + 3,
        ),
        _ => None,
    }
}

fn skip_value(input: &[u8], start: usize) -> Option<usize> {
    let mut pending = [0u16; DEPTH];
    let mut depth = 0usize;
    let mut cursor = start;
    loop {
        match *input.get(cursor)? {
            0 => cursor += 1,
            1 => {
                input.get(cursor..cursor + 2)?;
                cursor += 2;
            }
            2 => {
                if depth == DEPTH {
                    return None;
                }
                let count = u16::from_le_bytes(input.get(cursor + 1..cursor + 3)?.try_into().ok()?);
                cursor += 3;
                if count != 0 {
                    pending[depth] = count;
                    depth += 1;
                    continue;
                }
            }
            _ => return None,
        }
        loop {
            if depth == 0 {
                return Some(cursor);
            }
            pending[depth - 1] -= 1;
            if pending[depth - 1] != 0 {
                break;
            }
            depth -= 1;
        }
    }
}

struct Input {
    bytes: [u8; 512],
    len: usize,
}

impl Input {
    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[inline(never)]
fn opaque_input(seed: u64, periodic: bool) -> Input {
    input(seed, periodic)
}

fn input(seed: u64, periodic: bool) -> Input {
    let mut input = Input {
        bytes: [0; 512],
        len: 0,
    };
    push(&mut input, 2);
    push_bytes(&mut input, &COUNT.to_le_bytes());
    for index in 0..COUNT {
        let class = mix(seed ^ u64::from(index)) % 10;
        if periodic {
            if (index / 20) % 2 == 0 {
                push(&mut input, 1);
                push(&mut input, (seed as u8).wrapping_add(index as u8) & 1);
            } else {
                push(&mut input, 0);
            }
        } else if class == 0 {
            push(&mut input, 2);
            push_bytes(&mut input, &2u16.to_le_bytes());
            push(&mut input, 0);
            push(&mut input, 1);
            push(&mut input, (seed as u8).wrapping_add(index as u8) & 1);
        } else if class.is_multiple_of(2) {
            push(&mut input, 0);
        } else {
            push(&mut input, 1);
            push(&mut input, (seed as u8).wrapping_add(index as u8) & 1);
        }
    }
    input
}

fn push(input: &mut Input, value: u8) {
    input.bytes[input.len] = value;
    input.len += 1;
}

fn push_bytes(input: &mut Input, value: &[u8]) {
    input.bytes[input.len..input.len + value.len()].copy_from_slice(value);
    input.len += value.len();
}

fn mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
