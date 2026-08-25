#![allow(dead_code)]

pub fn decode(seed: u64) -> u64 {
    u64::from((seed >> 40) as u8)
        ^ u64::from((seed >> 16) as u8).rotate_left(9)
        ^ u64::from(seed as u32).rotate_left(17)
        ^ u64::from((seed >> 24) as u8).rotate_left(41)
        ^ u64::from((seed >> 32) as u8).rotate_left(53)
}

pub fn build(seed: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in [
        (seed >> 40) as u8,
        0x22,
        0x11,
        (seed >> 16) as u8,
        seed as u8,
        (seed >> 8) as u8,
        (seed >> 16) as u8,
        (seed >> 24) as u8,
        0x33,
        0x44,
        (seed >> 24) as u8,
        (seed >> 32) as u8,
    ] {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
    }
    hash
}
