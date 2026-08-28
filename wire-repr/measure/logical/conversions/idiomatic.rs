pub fn decode(seed: u64) -> u64 {
    let input = seed.to_le_bytes();
    let foo = u16::from_le_bytes(input[..2].try_into().unwrap());
    let bar = match input[2] {
        0 => false,
        1 => true,
        _ => return u64::MAX,
    };
    let Some(baz) = char::from_u32(u32::from_be_bytes(input[3..7].try_into().unwrap())) else {
        return u64::MAX;
    };
    u64::from(foo) ^ u64::from(bar) ^ u64::from(u32::from(baz)).rotate_left(23)
}

pub fn build(seed: u64) -> u64 {
    let Ok(foo) = u16::try_from(seed as usize) else {
        return u64::MAX;
    };
    let mut output = [0u8; 8];
    output[..2].copy_from_slice(&foo.to_le_bytes());
    output[2] = u8::from(seed & 1 != 0);
    output[3..7].copy_from_slice(&u32::from('λ').to_be_bytes());
    u64::from_le_bytes(output)
}

pub fn rust_decode(seed: u64) -> u64 {
    let input = seed.to_le_bytes();
    let id = u32::from_le_bytes(input[..4].try_into().unwrap());
    let count = u16::from_be_bytes(input[4..6].try_into().unwrap());
    if id == 0 || count == 0 {
        return u64::MAX;
    }
    u64::from(id) ^ u64::from(count).rotate_left(17)
}

pub fn rust_build(seed: u64) -> u64 {
    let id = seed as u32;
    let count = (seed >> 32) as u16;
    if id == 0 || count == 0 {
        return u64::MAX;
    }
    let mut output = [0u8; 8];
    output[..4].copy_from_slice(&id.to_le_bytes());
    output[4..6].copy_from_slice(&count.to_be_bytes());
    u64::from_le_bytes(output)
}
