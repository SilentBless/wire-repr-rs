#![allow(dead_code)]

pub fn decode(seed: u64) -> u64 {
    u64::from((seed >> 48) as u8)
        ^ u64::from(seed as u32 & 0x00ff_ffff).rotate_left(11)
        ^ u64::from((seed >> 24) as u16).rotate_left(31)
        ^ u64::from((seed as u32).rotate_left(11)).rotate_left(47)
}

pub fn build(seed: u64) -> u64 {
    let body = [seed as u8, (seed >> 8) as u8, (seed >> 16) as u8];
    let tail = ((seed >> 24) as u16).to_be_bytes();
    let rest = (seed as u32).rotate_left(11).to_le_bytes();
    let bytes = [
        6,
        3,
        (seed >> 48) as u8,
        0,
        0,
        0,
        body[0],
        body[1],
        body[2],
        0,
        0,
        0,
        tail[0],
        tail[1],
        rest[0],
        rest[1],
        rest[2],
        rest[3],
    ];
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
