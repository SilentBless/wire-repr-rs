#![allow(dead_code)]

pub fn decode(seed: u64) -> u64 {
    let mut value = u64::from((seed >> 32) as u8).rotate_left(53);
    for byte in [
        seed as u8,
        (seed >> 8) as u8,
        (seed >> 16) as u8,
        (seed >> 24) as u8,
    ] {
        value = value.rotate_left(7) ^ u64::from(byte);
    }
    value
}

pub fn build(seed: u64) -> u64 {
    hash(&input(seed))
}

pub fn copy(seed: u64) -> u64 {
    hash(&input(seed))
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

fn hash(bytes: &[u8; 9]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
