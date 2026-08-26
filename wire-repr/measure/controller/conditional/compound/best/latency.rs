#![allow(dead_code)]

pub fn decode(seed: u64) -> u64 {
    let present = seed & 1 == 0;
    2u64 ^ u64::from(seed as u16).rotate_left(7)
        ^ u64::from((seed >> 16) as u16).rotate_left(19)
        ^ u64::from(if present { (seed >> 32) as u8 } else { 0 }).rotate_left(31)
        ^ u64::from(if present { (seed >> 40) as u16 } else { 0 }).rotate_left(43)
        ^ u64::from((seed >> 56) as u8).rotate_left(59)
}

pub fn build(seed: u64) -> u64 {
    let first = (seed as u16).to_le_bytes();
    let second = ((seed >> 16) as u16).to_le_bytes();
    let present = seed & 1 == 0;
    let mut bytes = [0u8; 10];
    bytes[0] = u8::from(present);
    bytes[1] = 2;
    bytes[2..4].copy_from_slice(&first);
    bytes[4..6].copy_from_slice(&second);
    let len = if present {
        bytes[6] = (seed >> 32) as u8;
        bytes[7..9].copy_from_slice(&((seed >> 40) as u16).to_be_bytes());
        bytes[9] = (seed >> 56) as u8;
        10
    } else {
        bytes[6] = (seed >> 56) as u8;
        7
    };
    bytes[..len]
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
        })
}
