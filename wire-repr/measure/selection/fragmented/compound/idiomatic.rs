pub fn bytes(seed: u64) -> u64 {
    let input = input(seed);
    input[..1]
        .iter()
        .chain(&input[2..])
        .fold(0u64, |value, byte| value.rotate_left(5) ^ u64::from(*byte))
}

pub fn chunks(seed: u64) -> u64 {
    let input = input(seed);
    [&input[..2], &input[6..]]
        .into_iter()
        .fold(0u64, |value, chunk| {
            chunk
                .iter()
                .fold(value ^ chunk.len() as u64, |value, byte| {
                    value.rotate_left(5) ^ u64::from(*byte)
                })
        })
}

pub fn nested_bytes(seed: u64) -> u64 {
    let input = nested_input(seed);
    input[4..6]
        .iter()
        .chain(&input[8..])
        .fold(0u64, |value, byte| value.rotate_left(5) ^ u64::from(*byte))
}

pub fn nested_chunks(seed: u64) -> u64 {
    let input = nested_input(seed);
    [&input[..4], &input[6..]]
        .into_iter()
        .fold(0u64, |value, chunk| {
            chunk
                .iter()
                .fold(value ^ chunk.len() as u64, |value, byte| {
                    value.rotate_left(5) ^ u64::from(*byte)
                })
        })
}

#[inline(never)]
fn nested_input(seed: u64) -> [u8; 9] {
    [
        7,
        5,
        seed as u8,
        2,
        (seed >> 8) as u8,
        (seed >> 16) as u8,
        (seed >> 24) as u8,
        (seed >> 32) as u8,
        (seed >> 40) as u8,
    ]
}

#[inline(never)]
fn input(seed: u64) -> [u8; 7] {
    [
        seed as u8,
        4,
        (seed >> 8) as u8,
        (seed >> 16) as u8,
        (seed >> 24) as u8,
        (seed >> 32) as u8,
        (seed >> 40) as u8,
    ]
}
