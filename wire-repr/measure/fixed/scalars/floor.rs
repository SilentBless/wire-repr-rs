pub fn decode(seed: u64) -> u64 {
    u64::from(seed as u32) ^ u64::from(((seed >> 32) as u32).swap_bytes()).rotate_left(17)
}

pub fn build(seed: u64) -> u64 {
    u64::from(seed as u32) | (u64::from(((seed >> 32) as u32).swap_bytes()) << 32)
}

pub fn constant_decode(seed: u64) -> u64 {
    u64::from(seed as u32 == 0x4433_2211)
}

pub fn constant_build(_seed: u64) -> u64 {
    0x4433_2211
}
