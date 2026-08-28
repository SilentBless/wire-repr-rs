#![allow(dead_code)]

#[inline(always)]
fn encoded(seed: u64) -> u64 {
    u64::from(seed as u32) | (u64::from(((seed >> 32) as u32).swap_bytes()) << 32)
}

pub fn fixed(seed: u64) -> u64 {
    encoded(seed)
}

pub fn growable(seed: u64) -> u64 {
    encoded(seed)
}

pub fn owned(seed: u64) -> u64 {
    encoded(seed)
}

pub fn callback(seed: u64) -> u64 {
    encoded(seed)
}
