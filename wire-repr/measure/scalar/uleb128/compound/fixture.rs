#![allow(dead_code)]

pub const MAX_PAYLOAD: usize = 384;
pub const CAPACITY: usize = 2 + 3 + 5 + 10 + 19 + 3 + MAX_PAYLOAD + 2;

#[derive(Clone, Copy)]
pub struct Values {
    pub byte: u8,
    pub short: u16,
    pub word: u32,
    pub long: u64,
    pub wide: u128,
}

pub fn values(seed: u64) -> Values {
    Values {
        byte: seed as u8,
        short: seed.rotate_left(11) as u16,
        word: seed.rotate_left(23) as u32,
        long: seed.rotate_left(37),
        wide: u128::from(seed) | (u128::from(seed.rotate_left(29)) << 64),
    }
}

pub fn tail(seed: u64) -> u16 {
    (seed.rotate_left(17) as u16) ^ 0x6d3b
}

pub fn payload(seed: u64) -> ([u8; MAX_PAYLOAD], usize) {
    let length = match seed & 7 {
        0 => 0,
        1 => 1,
        2 => 63,
        3 => 127,
        4 => 128,
        5 => 255,
        6 => 256,
        _ => MAX_PAYLOAD,
    };
    let mut bytes = [0u8; MAX_PAYLOAD];
    for (index, byte) in bytes[..length].iter_mut().enumerate() {
        *byte = (seed.rotate_left((index as u32) & 63) as u8)
            .wrapping_add((index as u8).wrapping_mul(29));
    }
    (bytes, length)
}

// This fixture encoder writes a known number of base-128 digits directly. Neither
// handwritten role uses it to decode or build the measured representation.
#[inline(never)]
pub fn source(seed: u64) -> ([u8; CAPACITY], usize) {
    let values = values(seed);
    let (payload, length) = payload(seed);
    let nonminimal = seed & 1 != 0;
    let mut bytes = [0u8; CAPACITY];
    let mut end = 0;
    source_scalar(&mut bytes, &mut end, u128::from(values.byte), 8, nonminimal);
    source_scalar(
        &mut bytes,
        &mut end,
        u128::from(values.short),
        16,
        nonminimal,
    );
    source_scalar(
        &mut bytes,
        &mut end,
        u128::from(values.word),
        32,
        nonminimal,
    );
    source_scalar(
        &mut bytes,
        &mut end,
        u128::from(values.long),
        64,
        nonminimal,
    );
    source_scalar(&mut bytes, &mut end, values.wide, 128, nonminimal);
    source_scalar(&mut bytes, &mut end, length as u128, 16, nonminimal);
    bytes[end..end + length].copy_from_slice(&payload[..length]);
    end += length;
    bytes[end..end + 2].copy_from_slice(&tail(seed).to_le_bytes());
    (bytes, end + 2)
}

fn source_scalar(
    output: &mut [u8; CAPACITY],
    end: &mut usize,
    value: u128,
    bits: u32,
    nonminimal: bool,
) {
    let shortest = (128 - value.leading_zeros()).max(1).div_ceil(7) as usize;
    let maximum = bits.div_ceil(7) as usize;
    let groups = shortest + usize::from(nonminimal && shortest < maximum);
    for index in 0..groups {
        let digit = ((value >> (index * 7)) & 0x7f) as u8;
        output[*end + index] = digit | if index + 1 < groups { 0x80 } else { 0 };
    }
    *end += groups;
}

pub fn observe(values: Values, payload: &[u8], tail: u16) -> u64 {
    let numeric = u64::from(values.byte)
        ^ u64::from(values.short).rotate_left(7)
        ^ u64::from(values.word).rotate_left(19)
        ^ values.long.rotate_left(31)
        ^ (values.wide as u64).rotate_left(43)
        ^ ((values.wide >> 64) as u64).rotate_left(53)
        ^ u64::from(tail).rotate_left(13)
        ^ (payload.len() as u64).rotate_left(3);
    payload.iter().fold(numeric, |digest, byte| {
        digest.rotate_left(5) ^ u64::from(*byte)
    })
}

pub fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
