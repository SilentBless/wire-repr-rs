#![allow(dead_code)]

pub const MAX_PAYLOAD: usize = 384;
pub const CAPACITY: usize = 10 + 5 + 17 + 17 + 17 + MAX_PAYLOAD + 2;

#[derive(Clone, Copy)]
pub struct Values {
    pub signed: i64,
    pub transformed: i32,
    pub prefix: u64,
    pub signed_prefix: i128,
}

pub fn values(seed: u64) -> Values {
    Values {
        signed: match seed & 15 {
            0 => 0,
            1 => -1,
            2 => 63,
            3 => -64,
            4 => 64,
            5 => -65,
            6 => i64::MIN,
            7 => i64::MAX,
            _ => seed.rotate_left(23) as i64,
        },
        transformed: match seed & 15 {
            0 => 0,
            1 => -1,
            2 => 63,
            3 => -64,
            4 => 8192,
            5 => -8192,
            6 => i32::MIN,
            7 => i32::MAX,
            _ => seed.rotate_left(11) as i32,
        },
        prefix: match seed & 15 {
            0 => 0,
            1 => 127,
            2 => 128,
            3 => 16383,
            4 => 16384,
            5 => 2097151,
            6 => 2097152,
            7 => 0x0fffffff,
            8 => 0x10000000,
            9 => 0x12345678,
            10 => u64::MAX,
            _ => seed.rotate_left(37),
        },
        signed_prefix: match seed & 15 {
            0 => 0,
            1 => -1,
            2 => 1,
            3 => -64,
            4 => 64,
            5 => -8192,
            6 => 8192,
            7 => i128::MIN,
            8 => i128::MAX,
            _ => (u128::from(seed) | (u128::from(seed.rotate_left(29)) << 64)) as i128,
        },
    }
}

pub fn tail(seed: u64) -> u16 {
    (seed.rotate_left(17) as u16) ^ 0x6d3b
}

pub fn payload(seed: u64) -> ([u8; MAX_PAYLOAD], usize) {
    let length = [0, 1, 63, 127, 128, 255, 256, MAX_PAYLOAD][(seed & 7) as usize];
    let mut bytes = [0u8; MAX_PAYLOAD];
    for (index, byte) in bytes[..length].iter_mut().enumerate() {
        *byte = (seed.rotate_left((index as u32) & 63) as u8)
            .wrapping_add((index as u8).wrapping_mul(29));
    }
    (bytes, length)
}

// The fixture emits a counted number of groups, not a measured codec. Every
// measured role owns its decoder and shortest-form writer independently.
#[inline(never)]
pub fn source(seed: u64) -> ([u8; CAPACITY], usize) {
    let values = values(seed);
    let (payload, length) = payload(seed);
    let nonminimal = seed & 1 != 0;
    let mut bytes = [0u8; CAPACITY];
    let mut end = 0;
    source_signed(&mut bytes, &mut end, values.signed, nonminimal);
    let zigzag = (values.transformed as u32).wrapping_shl(1) ^ ((values.transformed >> 31) as u32);
    source_unsigned(&mut bytes, &mut end, zigzag, nonminimal);
    source_prefix(&mut bytes, &mut end, u128::from(values.prefix), nonminimal);
    let zigzag =
        (values.signed_prefix as u128).wrapping_shl(1) ^ ((values.signed_prefix >> 127) as u128);
    source_prefix(&mut bytes, &mut end, zigzag, nonminimal);
    source_prefix(&mut bytes, &mut end, length as u128, nonminimal);
    bytes[end..end + length].copy_from_slice(&payload[..length]);
    end += length;
    bytes[end..end + 2].copy_from_slice(&tail(seed).to_le_bytes());
    (bytes, end + 2)
}

fn source_signed(output: &mut [u8], end: &mut usize, value: i64, nonminimal: bool) {
    let significant = if value < 0 {
        65 - (!value as u64).leading_zeros()
    } else {
        65 - (value as u64).leading_zeros()
    };
    let shortest = significant.div_ceil(7) as usize;
    let groups = shortest + usize::from(nonminimal && shortest < 10);
    for index in 0..groups {
        let digit = ((i128::from(value) >> (index * 7)) & 0x7f) as u8;
        output[*end + index] = digit | if index + 1 < groups { 0x80 } else { 0 };
    }
    *end += groups;
}

fn source_unsigned(output: &mut [u8], end: &mut usize, value: u32, nonminimal: bool) {
    let shortest = (32 - value.leading_zeros()).max(1).div_ceil(7) as usize;
    let groups = shortest + usize::from(nonminimal && shortest < 5);
    for index in 0..groups {
        let digit = ((u64::from(value) >> (index * 7)) & 0x7f) as u8;
        output[*end + index] = digit | if index + 1 < groups { 0x80 } else { 0 };
    }
    *end += groups;
}

fn source_prefix(output: &mut [u8], end: &mut usize, value: u128, nonminimal: bool) {
    if nonminimal || value >= 1 << 28 {
        let payload = if nonminimal {
            16
        } else {
            (128 - value.leading_zeros()).div_ceil(8) as usize
        };
        output[*end] = 0xf0 | (payload as u8 - 1);
        output[*end + 1..*end + 1 + payload].copy_from_slice(&value.to_le_bytes()[..payload]);
        *end += payload + 1;
        return;
    }
    let (length, first_bits, prefix) = if value < 1 << 7 {
        (1, 7, 0)
    } else if value < 1 << 14 {
        (2, 6, 0x80)
    } else if value < 1 << 21 {
        (3, 5, 0xc0)
    } else {
        (4, 4, 0xe0)
    };
    output[*end] = prefix | (value & ((1 << first_bits) - 1)) as u8;
    for index in 1..length {
        output[*end + index] = (value >> (first_bits + (index - 1) * 8)) as u8;
    }
    *end += length;
}

pub fn observe(values: Values, payload: &[u8], tail: u16) -> u64 {
    let numeric = (values.signed as u64)
        ^ (values.transformed as u64).rotate_left(11)
        ^ values.prefix.rotate_left(23)
        ^ (values.signed_prefix as u64).rotate_left(37)
        ^ ((values.signed_prefix as u128 >> 64) as u64).rotate_left(49)
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
