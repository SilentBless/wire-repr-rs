#![allow(dead_code)]

pub fn decode(seed: u64) -> u64 {
    let input = input(seed);
    let offset = usize::from(input[0]);
    let len = usize::from(input[1]);
    let body = &input[offset..offset + len];
    let tail_start = align(offset + len, 4);
    let tail = u16::from_be_bytes(input[tail_start..tail_start + 2].try_into().unwrap());
    combine(input[2], body, tail, &input[tail_start + 2..])
}

pub fn build(seed: u64) -> u64 {
    hash(&input(seed))
}

fn align(position: usize, alignment: usize) -> usize {
    let remainder = position % alignment;
    if remainder == 0 {
        position
    } else {
        position + alignment - remainder
    }
}

fn body(seed: u64) -> [u8; 3] {
    [seed as u8, (seed >> 8) as u8, (seed >> 16) as u8]
}

fn rest(seed: u64) -> [u8; 4] {
    (seed as u32).rotate_left(11).to_le_bytes()
}

fn input(seed: u64) -> [u8; 18] {
    let mut input = [0u8; 18];
    input[0] = 6;
    input[1] = 3;
    input[2] = (seed >> 48) as u8;
    input[6..9].copy_from_slice(&body(seed));
    input[12..14].copy_from_slice(&((seed >> 24) as u16).to_be_bytes());
    input[14..18].copy_from_slice(&rest(seed));
    input
}

fn combine(head: u8, body: &[u8], tail: u16, rest: &[u8]) -> u64 {
    u64::from(head)
        ^ u64::from(u32::from_le_bytes([body[0], body[1], body[2], 0])).rotate_left(11)
        ^ u64::from(tail).rotate_left(31)
        ^ u64::from(u32::from_le_bytes(rest.try_into().unwrap())).rotate_left(47)
}

fn hash(bytes: &[u8; 18]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
