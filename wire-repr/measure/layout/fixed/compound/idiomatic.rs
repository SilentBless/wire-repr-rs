#![allow(dead_code)]

pub fn decode(seed: u64) -> u64 {
    let input = input(seed);
    if u16::from_le_bytes([input[1], input[2]]) != 0x1122
        || u16::from_be_bytes([input[8], input[9]]) != 0x3344
    {
        return u64::MAX;
    }
    combine(
        input[0],
        input[3],
        input[4..8].try_into().unwrap(),
        input[10],
        input[11],
    )
}

pub fn build(seed: u64) -> u64 {
    hash(&input(seed))
}

fn input(seed: u64) -> [u8; 12] {
    let mut output = [0u8; 12];
    output[0] = (seed >> 40) as u8;
    output[1..3].copy_from_slice(&0x1122u16.to_le_bytes());
    output[3] = (seed >> 16) as u8;
    output[4..8].copy_from_slice(&(seed as u32).to_le_bytes());
    output[8..10].copy_from_slice(&0x3344u16.to_be_bytes());
    output[10] = (seed >> 24) as u8;
    output[11] = (seed >> 32) as u8;
    output
}

fn combine(header: u8, first: u8, bytes: [u8; 4], second: u8, trailer: u8) -> u64 {
    u64::from(header)
        ^ u64::from(first).rotate_left(9)
        ^ u64::from(u32::from_le_bytes(bytes)).rotate_left(17)
        ^ u64::from(second).rotate_left(41)
        ^ u64::from(trailer).rotate_left(53)
}

fn hash(bytes: &[u8; 12]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
