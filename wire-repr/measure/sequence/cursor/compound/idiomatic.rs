pub fn fixed(seed: u64) -> u64 {
    let input = fixed_input(seed);
    input.chunks_exact(2).fold(0u64, |sum, item| {
        sum.wrapping_add(u64::from(u16::from_le_bytes([item[0], item[1]])))
    })
}

pub fn variable(seed: u64) -> u64 {
    let input = variable_input(seed);
    let mut offset = 0usize;
    let mut sum = 0u64;
    while offset < input.len() {
        let length = usize::from(input[offset]);
        let end = offset + 1 + length;
        if end > input.len() {
            return u64::MAX;
        }
        sum = input[offset + 1..end]
            .iter()
            .fold(sum, |sum, byte| sum.rotate_left(5) ^ u64::from(*byte));
        offset = end;
    }
    sum
}

pub fn cursor(seed: u64) -> u64 {
    let input = cursor_input(seed);
    let length = usize::from(input[1]);
    let end = 2 + length;
    if end > input.len() {
        return u64::MAX;
    }
    u64::from(input[0])
        ^ input[2..end]
            .iter()
            .fold(0u64, |value, byte| value.rotate_left(5) ^ u64::from(*byte))
        ^ (input.len() - end) as u64
}

#[inline(never)]
fn fixed_input(seed: u64) -> [u8; 128] {
    let mut output = [0u8; 128];
    let mut index = 0usize;
    while index < 64 {
        let value = (seed as u16).wrapping_add(index as u16).to_le_bytes();
        output[index * 2] = value[0];
        output[index * 2 + 1] = value[1];
        index += 1;
    }
    output
}

#[inline(never)]
fn variable_input(seed: u64) -> [u8; 128] {
    let mut output = [0u8; 128];
    let mut cursor = 0usize;
    let mut index = 0usize;
    while index < 32 {
        let length = [1usize, 5, 2, 4][index % 4];
        output[cursor] = length as u8;
        let mut item = 0usize;
        while item < length {
            output[cursor + 1 + item] = seed.wrapping_add(index as u64 + item as u64) as u8;
            item += 1;
        }
        cursor += 1 + length;
        index += 1;
    }
    output
}

#[inline(never)]
fn cursor_input(seed: u64) -> [u8; 7] {
    let length = ((seed >> 3) as usize % 4 + 1) as u8;
    [
        seed as u8,
        length,
        (seed >> 8) as u8,
        (seed >> 16) as u8,
        (seed >> 24) as u8,
        (seed >> 32) as u8,
        9,
    ]
}
