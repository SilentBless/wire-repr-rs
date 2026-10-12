#![allow(dead_code)]

pub const CAPACITY: usize = 22;

pub struct Values {
    pub first: [u8; 7],
    pub first_len: usize,
    pub middle: u16,
    pub second: [u8; 7],
    pub second_len: usize,
    pub tail: u32,
}

pub fn values(seed: u64) -> Values {
    Values {
        first: core::array::from_fn(|index| seed.rotate_right((index * 7) as u32) as u8),
        first_len: (seed & 7) as usize,
        middle: (seed >> 16) as u16,
        second: core::array::from_fn(|index| seed.rotate_left((index * 9) as u32) as u8),
        second_len: ((seed >> 8) & 7) as usize,
        tail: (seed >> 32) as u32,
    }
}

pub struct Input {
    pub bytes: [u8; CAPACITY],
    pub len: usize,
}

impl Input {
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

pub fn encoded(seed: u64) -> Input {
    let values = values(seed);
    let mut bytes = [0u8; CAPACITY];
    bytes[0] = values.first_len as u8;
    bytes[1..1 + values.first_len].copy_from_slice(&values.first[..values.first_len]);
    let mut cursor = 1 + values.first_len;
    bytes[cursor..cursor + 2].copy_from_slice(&values.middle.to_be_bytes());
    cursor += 2;
    bytes[cursor] = values.second_len as u8;
    cursor += 1;
    bytes[cursor..cursor + values.second_len].copy_from_slice(&values.second[..values.second_len]);
    cursor += values.second_len;
    bytes[cursor..cursor + 4].copy_from_slice(&values.tail.to_le_bytes());
    Input {
        bytes,
        len: cursor + 4,
    }
}

#[inline(never)]
pub fn input(seed: u64) -> Input {
    encoded(seed)
}

pub fn domain_input(seed: u64) -> Input {
    let mut input = input(seed);
    match seed & 3 {
        0 => input.len -= 1,
        1 => input.bytes[0] = u8::MAX,
        2 if input.len < CAPACITY => input.len += 1,
        _ => {}
    }
    input
}

#[inline(always)]
pub fn combine(first: &[u8], middle: u16, second: &[u8], tail: u32) -> u64 {
    let first = hash(first);
    let second = hash(second);
    first ^ second.rotate_left(17) ^ u64::from(middle).rotate_left(31) ^ u64::from(tail)
}

#[inline(always)]
pub fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
