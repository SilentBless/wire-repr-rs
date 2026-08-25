pub fn decode(seed: u64) -> u64 {
    let foo = seed as u32;
    let bar = (seed >> 32) as u32;
    if bar == 0x4433_2211 {
        u64::from(foo) + u64::from(bar)
    } else {
        u64::MAX
    }
}

pub fn build(seed: u64) -> u64 {
    u64::from(seed as u32) | 0x4433_2211_0000_0000
}
