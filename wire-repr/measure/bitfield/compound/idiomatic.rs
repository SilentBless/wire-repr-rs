pub fn decode(seed: u64) -> u64 {
    let raw = seed as u16;
    u64::from(raw & 1 != 0)
        | (u64::from((raw >> 1) & 7) << 1)
        | (u64::from((raw >> 8) & 15) << 8)
        | (u64::from(raw & 1 != 0) << 16)
        | (u64::from((raw >> 1) & 7) << 17)
        | (u64::from((seed >> 16) as u8) << 24)
}

pub fn build(seed: u64) -> u64 {
    let foo = u16::from(seed & 1 != 0)
        | ((((seed >> 1) & 7) as u16) << 1)
        | ((((seed >> 8) & 15) as u16) << 8);
    let bar = u16::from((seed >> 16) & 1 != 0) | ((((seed >> 17) & 7) as u16) << 1);
    let foo = foo.to_be_bytes();
    let bar = bar.to_le_bytes();
    hash(&[foo[0], foo[1], bar[0], bar[1], (seed >> 24) as u8])
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
