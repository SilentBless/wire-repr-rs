pub fn decode(seed: u64) -> u64 {
    let input = seed.to_le_bytes();
    let foo = u32::from_le_bytes(input[..4].try_into().unwrap());
    let bar = u32::from_be_bytes(input[4..].try_into().unwrap());
    u64::from(foo) ^ u64::from(bar).rotate_left(17)
}

pub fn build(seed: u64) -> u64 {
    let mut output = [0u8; 8];
    output[..4].copy_from_slice(&(seed as u32).to_le_bytes());
    output[4..].copy_from_slice(&((seed >> 32) as u32).to_be_bytes());
    u64::from_le_bytes(output)
}

pub fn constant_decode(seed: u64) -> u64 {
    u64::from(seed as u32 == 0x4433_2211)
}

pub fn constant_build(_seed: u64) -> u64 {
    u64::from(0x4433_2211u32)
}
