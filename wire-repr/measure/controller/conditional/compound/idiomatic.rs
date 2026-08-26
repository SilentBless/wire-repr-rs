#![allow(dead_code)]

pub fn decode(seed: u64) -> u64 {
    let (input, len) = input(seed);
    try_decode(&input[..len]).unwrap_or(u64::MAX)
}

fn try_decode(input: &[u8]) -> Option<u64> {
    let present = match *input.first()? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let length = usize::from(*input.get(1)?);
    let mut cursor = 2usize;
    let [first_0, first_1] = input.get(cursor..cursor.checked_add(length)?)? else {
        return None;
    };
    cursor += length;
    let [second_0, second_1] = input.get(cursor..cursor.checked_add(length)?)? else {
        return None;
    };
    cursor += length;
    let first = u16::from_le_bytes([*first_0, *first_1]);
    let second = u16::from_le_bytes([*second_0, *second_1]);
    let (value, code) = if present {
        let value = *input.get(cursor)?;
        let code = u16::from_be_bytes([*input.get(cursor + 1)?, *input.get(cursor + 2)?]);
        cursor += 3;
        (value, code)
    } else {
        (0, 0)
    };
    let tail = *input.get(cursor)?;
    if cursor + 1 != input.len() {
        return None;
    }
    Some(
        u64::from(length as u8)
            ^ u64::from(first).rotate_left(7)
            ^ u64::from(second).rotate_left(19)
            ^ u64::from(value).rotate_left(31)
            ^ u64::from(code).rotate_left(43)
            ^ u64::from(tail).rotate_left(59),
    )
}

pub fn build(seed: u64) -> u64 {
    let (input, len) = input(seed);
    hash(&input[..len])
}

fn input(seed: u64) -> ([u8; 10], usize) {
    let first = (seed as u16).to_le_bytes();
    let second = ((seed >> 16) as u16).to_le_bytes();
    let present = seed & 1 == 0;
    let mut input = [0u8; 10];
    input[0] = u8::from(present);
    input[1] = 2;
    input[2..4].copy_from_slice(&first);
    input[4..6].copy_from_slice(&second);
    if present {
        input[6] = (seed >> 32) as u8;
        input[7..9].copy_from_slice(&((seed >> 40) as u16).to_be_bytes());
        input[9] = (seed >> 56) as u8;
        (input, 10)
    } else {
        input[6] = (seed >> 56) as u8;
        (input, 7)
    }
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
