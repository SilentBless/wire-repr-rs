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

pub fn array_decode(seed: u64) -> u64 {
    let bytes = seed.to_le_bytes();
    (0..4).fold(0u64, |hash, index| {
        let value = u16::from_le_bytes([bytes[index * 2], bytes[index * 2 + 1]]);
        hash.rotate_left(11) ^ u64::from(value)
    })
}

pub fn array_build(seed: u64) -> u64 {
    let bytes = seed.to_le_bytes();
    let values: [u16; 4] =
        core::array::from_fn(|index| u16::from_le_bytes([bytes[index * 2], bytes[index * 2 + 1]]));
    let mut output = [0u8; 8];
    for (index, value) in values.into_iter().enumerate() {
        output[index * 2..index * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
    u64::from_le_bytes(output)
}
