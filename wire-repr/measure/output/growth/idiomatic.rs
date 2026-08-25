#![allow(dead_code)]

pub fn fixed(seed: u64) -> u64 {
    let mut output = [0u8; 8];
    output[..4].copy_from_slice(&(seed as u32).to_le_bytes());
    output[4..].copy_from_slice(&((seed >> 32) as u32).to_be_bytes());
    u64::from_le_bytes(output)
}

pub fn growable(seed: u64) -> u64 {
    let mut output = Vec::with_capacity(8);
    output.extend_from_slice(&(seed as u32).to_le_bytes());
    output.extend_from_slice(&((seed >> 32) as u32).to_be_bytes());
    let bytes: [u8; 8] = output.as_slice().try_into().unwrap_or([0xff; 8]);
    u64::from_le_bytes(bytes)
}

pub fn callback(seed: u64) -> u64 {
    let mut output = [0u8; 16];
    output[..4].copy_from_slice(&(seed as u32).to_le_bytes());
    output[4..8].copy_from_slice(&((seed >> 32) as u32).to_be_bytes());
    u64::from_le_bytes(output[..8].try_into().unwrap_or([0xff; 8]))
}
