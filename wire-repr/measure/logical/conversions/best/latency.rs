pub fn decode(seed: u64) -> u64 {
    let foo = seed as u16;
    let bar = match ((seed >> 16) & 0xff) as u8 {
        0 => false,
        1 => true,
        _ => return u64::MAX,
    };
    let bytes = seed.to_le_bytes();
    let Some(baz) = char::from_u32(u32::from_be_bytes([bytes[3], bytes[4], bytes[5], bytes[6]]))
    else {
        return u64::MAX;
    };
    u64::from(foo) ^ u64::from(bar) ^ u64::from(u32::from(baz)).rotate_left(23)
}

pub fn build(seed: u64) -> u64 {
    if seed > u16::MAX.into() {
        return u64::MAX;
    }
    let foo = seed as u16;
    let bar = u8::from(seed & 1 != 0);
    let baz = u32::from('λ').to_be_bytes();
    u64::from(foo) | (u64::from(bar) << 16) | (u64::from(u32::from_le_bytes(baz)) << 24)
}

pub fn rust_decode(seed: u64) -> u64 {
    let id = seed as u32;
    let bytes = seed.to_le_bytes();
    let count = u16::from_be_bytes([bytes[4], bytes[5]]);
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
    u64::from(id) | (u64::from(count.swap_bytes()) << 32)
}
