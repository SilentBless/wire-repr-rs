pub fn decode(seed: u64) -> u64 {
    let input = seed.to_le_bytes();
    let foo = u32::from_le_bytes(input[..4].try_into().unwrap());
    let bar = u32::from_le_bytes(input[4..].try_into().unwrap());
    if bar == 0x4433_2211 {
        u64::from(foo) + u64::from(bar)
    } else {
        u64::MAX
    }
}

pub fn build(seed: u64) -> u64 {
    u64::from(seed as u32) | (u64::from(0x4433_2211u32) << 32)
}
