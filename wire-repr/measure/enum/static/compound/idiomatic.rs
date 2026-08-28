pub fn decode(seed: u64) -> u64 {
    let input = input(seed);
    match u16::from_le_bytes([input[0], input[1]]) {
        1 => u64::from(u32::from_le_bytes([input[2], input[3], input[4], input[5]])).rotate_left(7),
        2 => u64::from(input[2]) | (u64::from(input[3]) << 8),
        selector => input[2..].iter().fold(u64::from(selector), |value, byte| {
            value.rotate_left(5) ^ u64::from(*byte)
        }),
    }
}

pub fn build(seed: u64) -> u64 {
    let mut output = [0u8; 6];
    let length = match seed % 3 {
        0 => {
            output[..2].copy_from_slice(&1u16.to_le_bytes());
            output[2..6].copy_from_slice(&(seed as u32).to_le_bytes());
            6
        }
        1 => {
            output[..2].copy_from_slice(&2u16.to_le_bytes());
            output[2] = seed as u8;
            output[3] = (seed >> 8) as u8;
            6
        }
        _ => {
            output[..2].copy_from_slice(&7u16.to_le_bytes());
            output[2] = seed as u8;
            output[3] = (seed >> 8) as u8;
            output[4] = (seed >> 16) as u8;
            6
        }
    };
    hash(&output[..length])
}

pub fn copy(seed: u64) -> u64 {
    let source = input(seed);
    if validate(&source).is_none() {
        return u64::MAX;
    }
    let mut output = [0u8; 7];
    output[0] = 1;
    output[1..].copy_from_slice(&source);
    hash(&output)
}

pub fn unit_decode(seed: u64) -> u64 {
    let (input, len) = unit_input(seed);
    match (input[0], len) {
        (0, 1) => 0,
        (1, 5) => u64::from(u32::from_le_bytes(input[1..].try_into().unwrap())),
        _ => u64::MAX,
    }
}

pub fn unit_build(seed: u64) -> u64 {
    let mut output = [0u8; 5];
    let len = if seed & 1 == 0 {
        output[0] = 0;
        1
    } else {
        output[0] = 1;
        output[1..].copy_from_slice(&(seed as u32).to_le_bytes());
        5
    };
    hash(&output[..len])
}

#[inline(never)]
fn unit_input(seed: u64) -> ([u8; 5], usize) {
    if seed & 1 == 0 {
        ([0; 5], 1)
    } else {
        let value = (seed as u32).to_le_bytes();
        ([1, value[0], value[1], value[2], value[3]], 5)
    }
}

fn validate(source: &[u8]) -> Option<()> {
    let selector = u16::from_le_bytes([*source.first()?, *source.get(1)?]);
    let valid = match selector {
        1 | 2 => source.len() == 6,
        _ => source.len() >= 2,
    };
    valid.then_some(())
}

#[inline(never)]
fn input(seed: u64) -> [u8; 6] {
    match seed % 3 {
        0 => {
            let value = (seed as u32).to_le_bytes();
            [1, 0, value[0], value[1], value[2], value[3]]
        }
        1 => [2, 0, seed as u8, (seed >> 8) as u8, 0, 0],
        _ => [7, 0, seed as u8, (seed >> 8) as u8, (seed >> 16) as u8, 0],
    }
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
