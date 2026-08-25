const ROOT_TAG: u32 = 0x1122_3344;
const LEAF_TAG: u32 = 0x5566_7788;

pub fn decode(seed: u64) -> u64 {
    let input = input(seed);
    let tag = u32::from_le_bytes(input[..4].try_into().unwrap());
    let value = u32::from_le_bytes(input[4..8].try_into().unwrap());
    let nested = i16::from_be_bytes(input[8..10].try_into().unwrap());
    let byte = input[10];
    let leaf = u32::from_le_bytes(input[11..15].try_into().unwrap());
    let enabled = match input[15] {
        0 => false,
        1 => true,
        _ => return u64::MAX,
    };
    if tag != ROOT_TAG || leaf != LEAF_TAG {
        return u64::MAX;
    }
    combine(tag, value, nested, byte, leaf, enabled)
}

pub fn build(seed: u64) -> u64 {
    hash(&input(seed))
}

fn input(seed: u64) -> [u8; 16] {
    let mut input = [0u8; 16];
    input[..4].copy_from_slice(&ROOT_TAG.to_le_bytes());
    input[4..8].copy_from_slice(&(seed as u32).to_le_bytes());
    input[8..10].copy_from_slice(&((seed >> 32) as i16).to_be_bytes());
    input[10] = (seed >> 48) as u8;
    input[11..15].copy_from_slice(&LEAF_TAG.to_le_bytes());
    input[15] = u8::from(seed & 1 != 0);
    input
}

fn combine(tag: u32, value: u32, nested: i16, byte: u8, leaf: u32, enabled: bool) -> u64 {
    u64::from(tag)
        ^ u64::from(value).rotate_left(7)
        ^ u64::from(nested as u16).rotate_left(19)
        ^ u64::from(byte).rotate_left(31)
        ^ u64::from(leaf).rotate_left(41)
        ^ u64::from(enabled)
}

fn hash(bytes: &[u8; 16]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
