pub fn fixed(seed: u64) -> u64 {
    let first = seed as u8;
    let tail = (seed >> 8) as u8;
    let second = u16::from(first) + u16::from(tail);
    let third = second * 2;
    let second = second.to_le_bytes();
    let third = third.to_le_bytes();
    hash(&[first, third[0], third[1], second[0], second[1], tail])
}

pub fn dynamic(seed: u64) -> u64 {
    let body = [seed as u8, (seed >> 8) as u8, (seed >> 16) as u8];
    let checksum = 3u16 + body.iter().map(|byte| u16::from(*byte)).sum::<u16>();
    let checksum = checksum.to_le_bytes();
    hash(&[checksum[0], checksum[1], 3, body[0], body[1], body[2]])
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
