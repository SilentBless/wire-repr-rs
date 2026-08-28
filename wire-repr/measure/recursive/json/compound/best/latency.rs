#![allow(dead_code)]

const COUNT: u16 = 200;
const DEPTH: usize = 64;

pub fn pair_build(seed: u64) -> u64 {
    let depth = 4 + seed as usize % 8;
    let mut output = [0u8; 256];
    output[..depth].fill(4);
    output[depth..depth + 2].copy_from_slice(&[1, seed as u8]);
    let mut cursor = depth + 2;
    for level in 1..=depth {
        output[cursor] = (seed as u8) ^ level as u8;
        output[cursor + 1] = 1;
        output[cursor + 2] = (seed as u8).wrapping_add(level as u8);
        cursor += 3;
    }
    hash_bytes(&output[..cursor])
}

pub fn array_build(seed: u64) -> u64 {
    let count = 16 + seed as usize % 16;
    let mut output = [0u8; 256];
    output[0] = 2;
    output[1..3].copy_from_slice(&(count as u16).to_le_bytes());
    for index in 0..count {
        let cursor = 3 + 2 * index;
        output[cursor] = 1;
        output[cursor + 1] = (seed as u8).wrapping_add(index as u8);
    }
    hash_bytes(&output[..3 + 2 * count])
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}

pub fn demand_view(seed: u64) -> u64 {
    let input = opaque_demand_input(seed);
    let metadata_len = seed as usize % 16;
    let opcode = 11 + metadata_len;
    hash_bytes(&input.bytes[11..opcode])
        ^ (u64::from((seed as u16).rotate_left(3)) << 32)
        ^ (u64::from(seed as u8) << 8)
        ^ u64::from((seed as u8).wrapping_add(1))
}

pub fn demand_build(seed: u64) -> u64 {
    let (metadata, metadata_len) = opaque_demand_metadata(seed);
    let mut output = [0u8; 64];
    output[0] = 6;
    output[1..9].copy_from_slice(&(metadata_len as u64).to_le_bytes());
    output[9] = 1;
    output[10] = seed as u8;
    output[11..11 + metadata_len].copy_from_slice(&metadata[..metadata_len]);
    let opcode = 11 + metadata_len;
    output[opcode..opcode + 2].copy_from_slice(&(seed as u16).rotate_left(3).to_le_bytes());
    output[opcode + 2] = 1;
    output[opcode + 3] = (seed as u8).wrapping_add(1);
    hash_bytes(&output[..opcode + 4])
}

#[inline(never)]
fn opaque_demand_metadata(seed: u64) -> ([u8; 16], usize) {
    let metadata_len = seed as usize % 16;
    let mut metadata = [0u8; 16];
    for index in 0..metadata_len {
        metadata[index] = (seed as u8).wrapping_mul(3).wrapping_add(index as u8);
    }
    (metadata, metadata_len)
}

#[inline(never)]

fn opaque_demand_input(seed: u64) -> PairInput {
    let metadata_len = seed as usize % 16;
    let mut input = PairInput {
        bytes: [0; 256],
        len: 15 + metadata_len,
    };
    input.bytes[0] = 6;
    input.bytes[1..9].copy_from_slice(&(metadata_len as u64).to_le_bytes());
    input.bytes[9] = 1;
    input.bytes[10] = seed as u8;
    for index in 0..metadata_len {
        input.bytes[11 + index] = (seed as u8).wrapping_mul(3).wrapping_add(index as u8);
    }
    let opcode = 11 + metadata_len;
    input.bytes[opcode..opcode + 2].copy_from_slice(&(seed as u16).rotate_left(3).to_le_bytes());
    input.bytes[opcode + 2] = 1;
    input.bytes[opcode + 3] = (seed as u8).wrapping_add(1);
    input
}

pub fn pair_view(seed: u64) -> u64 {
    let input = opaque_pair_input(seed);
    let depth = 16 + seed as usize % 16;
    let opcode = 4 * depth - 1;
    if input.len != 4 * depth + 2
        || input.bytes[..depth].iter().any(|selector| *selector != 4)
        || input.bytes[opcode + 1] != 1
    {
        return u64::MAX;
    }
    (u64::from(input.bytes[opcode]) << 32)
        | (u64::from(input.bytes[opcode + 2]) << 16)
        | input.len as u64
}

struct PairInput {
    bytes: [u8; 256],
    len: usize,
}

#[inline(never)]
fn opaque_pair_input(seed: u64) -> PairInput {
    let depth = 16 + seed as usize % 16;
    let mut input = PairInput {
        bytes: [0; 256],
        len: 0,
    };
    for _ in 0..depth {
        input.bytes[input.len] = 4;
        input.len += 1;
    }
    input.bytes[input.len..input.len + 2].copy_from_slice(&[1, seed as u8]);
    input.len += 2;
    for index in 0..depth {
        input.bytes[input.len..input.len + 3].copy_from_slice(&[
            index as u8,
            1,
            (seed as u8).wrapping_add(index as u8).wrapping_add(1),
        ]);
        input.len += 3;
    }
    input
}

pub fn direct_batch(seed: u64) -> u64 {
    let input = opaque_input(seed, true);
    let bytes = input.as_slice();
    if skip_value(bytes, 0) != Some(bytes.len()) {
        return u64::MAX;
    }
    let mut hash = seed;
    for query in 0..COUNT {
        let index = (usize::from(query) * 37 + seed as usize) % usize::from(COUNT);
        let block = index / 20;
        let within = index % 20;
        let start = 3
            + block / 2 * 60
            + if block % 2 == 0 {
                within * 2
            } else {
                40 + within
            };
        let Some(value) = observe(bytes, start) else {
            return u64::MAX;
        };
        hash = hash.rotate_left(7) ^ value;
    }
    hash
}

pub fn direct_lookup(seed: u64) -> u64 {
    lookup(seed, true).unwrap_or(u64::MAX)
}

pub fn mixed_lookup(seed: u64) -> u64 {
    lookup(seed, false).unwrap_or(u64::MAX)
}

fn lookup(seed: u64, periodic: bool) -> Option<u64> {
    let input = opaque_input(seed, periodic);
    let bytes = input.as_slice();
    let requested = seed as usize % usize::from(COUNT);
    let mut cursor = 3usize;
    let mut selected = None;
    for index in 0..usize::from(COUNT) {
        let start = cursor;
        cursor = skip_value(bytes, cursor)?;
        if index == requested {
            selected = observe(bytes, start);
        }
    }
    (cursor == bytes.len()).then_some(selected?)
}

pub fn iterate(seed: u64) -> u64 {
    let input = opaque_input(seed, false);
    let bytes = input.as_slice();
    let mut cursor = 3usize;
    let mut hash = seed;
    for _ in 0..COUNT {
        let start = cursor;
        cursor = match skip_value(bytes, cursor) {
            Some(cursor) => cursor,
            None => return u64::MAX,
        };
        let Some(value) = observe(bytes, start) else {
            return u64::MAX;
        };
        hash = hash.rotate_left(7) ^ value;
    }
    (cursor == bytes.len()).then_some(hash).unwrap_or(u64::MAX)
}

fn observe(input: &[u8], start: usize) -> Option<u64> {
    match *input.get(start)? {
        0 => Some(0),
        1 => Some(u64::from(*input.get(start + 1)?) + 1),
        2 => Some(
            u64::from(u16::from_le_bytes(
                input.get(start + 1..start + 3)?.try_into().ok()?,
            )) + 3,
        ),
        3 => Some(u64::from(*input.get(start + 1)?) + 5),
        _ => None,
    }
}

fn skip_value(input: &[u8], start: usize) -> Option<usize> {
    let mut pending = [0u16; DEPTH];
    let mut depth = 0usize;
    let mut cursor = start;
    loop {
        match *input.get(cursor)? {
            0 => cursor += 1,
            1 => {
                input.get(cursor..cursor + 2)?;
                cursor += 2;
            }
            2 => {
                if depth == DEPTH {
                    return None;
                }
                let count = u16::from_le_bytes(input.get(cursor + 1..cursor + 3)?.try_into().ok()?);
                cursor += 3;
                if count != 0 {
                    pending[depth] = count;
                    depth += 1;
                    continue;
                }
            }
            3 => {
                let length = usize::from(*input.get(cursor + 1)?);
                input.get(cursor..cursor + 2 + length)?;
                cursor += 2 + length;
            }
            _ => return None,
        }
        loop {
            if depth == 0 {
                return Some(cursor);
            }
            pending[depth - 1] -= 1;
            if pending[depth - 1] != 0 {
                break;
            }
            depth -= 1;
        }
    }
}

pub fn fixed_mode(seed: u64) -> u64 {
    mode_lookup(seed, 0)
}
pub fn formula_mode(seed: u64) -> u64 {
    mode_lookup(seed, 1)
}
pub fn interval_mode(seed: u64) -> u64 {
    mode_lookup(seed, 2)
}
pub fn ranked_mode(seed: u64) -> u64 {
    mode_lookup(seed, 3)
}
pub fn factorized_mode(seed: u64) -> u64 {
    mode_lookup(seed, 4)
}
pub fn factorized_fallback_mode(seed: u64) -> u64 {
    mode_lookup(seed, 9)
}
pub fn shape_mode(seed: u64) -> u64 {
    mode_lookup(seed, 5)
}
pub fn periodic_mode(seed: u64) -> u64 {
    mode_lookup(seed, 6)
}
pub fn packed_mode(seed: u64) -> u64 {
    mode_lookup(seed, 7)
}
pub fn replay_mode(seed: u64) -> u64 {
    mode_lookup(seed, 8)
}

fn mode_lookup(seed: u64, mode: u8) -> u64 {
    let input = opaque_mode_input(seed, mode);
    let bytes = input.as_slice();
    let count = u16::from_le_bytes([bytes[1], bytes[2]]) as usize;
    let requested = seed as usize % count;
    let mut cursor = 3usize;
    let mut selected = None;
    for index in 0..count {
        let start = cursor;
        let Some(end) = skip_value(bytes, cursor) else {
            return u64::MAX;
        };
        cursor = end;
        if index == requested {
            selected = observe(bytes, start);
        }
    }
    (cursor == bytes.len())
        .then_some(selected)
        .flatten()
        .unwrap_or(u64::MAX)
}

struct ModeInput {
    bytes: [u8; 32_768],
    len: usize,
}

impl ModeInput {
    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[inline(never)]
fn opaque_mode_input(seed: u64, mode: u8) -> ModeInput {
    mode_input(seed, mode)
}

fn mode_input(seed: u64, mode: u8) -> ModeInput {
    let mut input = ModeInput {
        bytes: [0; 32_768],
        len: 0,
    };
    push_mode(&mut input, 2);
    let count_offset = input.len;
    push_mode_bytes(&mut input, &[0; 2]);
    let count = match mode {
        0 => {
            for _ in 0..128 {
                push_mode(&mut input, 0);
            }
            128
        }
        1 => {
            for index in 0..128 {
                push_mode_width(&mut input, 3 + index);
            }
            128
        }
        2 => {
            let mut count = 0;
            for run in 0..12 {
                for _ in 0..=run {
                    push_mode_width(&mut input, 5 + run * 3);
                    count += 1;
                }
            }
            count
        }
        3 => {
            for index in 0..200 {
                push_mode_width(&mut input, 3 + (mix(seed ^ index) % 50) as usize);
            }
            200
        }
        4 => {
            for index in 0..300usize {
                let variant = 2 + ((index % 16) * 7) % 13;
                let depth_class = (index / 16) % 64;
                let depth = (depth_class * 5 + depth_class / 7) % 17;
                push_mode_width(&mut input, 8 + variant + depth);
            }
            300
        }
        5 => {
            for index in 0..300 {
                push_mode(&mut input, 2);
                if index % 2 == 0 {
                    push_mode_bytes(&mut input, &0u16.to_le_bytes());
                } else {
                    push_mode_bytes(&mut input, &1u16.to_le_bytes());
                    push_mode(&mut input, 0);
                }
            }
            300
        }
        6 => {
            for index in 0..200 {
                let boolean = (index / 20) % 2 == 0;
                push_mode(&mut input, if boolean { 1 } else { 0 });
                if boolean {
                    push_mode(&mut input, index as u8 & 1);
                }
            }
            200
        }
        7 => {
            let mut previous = usize::MAX;
            for run in 0..256usize {
                let mut class = (mix(seed ^ run as u64) % 50) as usize;
                if class == previous {
                    class = (class + 1) % 50;
                }
                previous = class;
                for _ in 0..2 {
                    push_mode_width(&mut input, 3 + class);
                }
            }
            512
        }
        8 => {
            for index in 0..600 {
                push_mode_width(&mut input, 3 + (mix(seed ^ index) % 40) as usize);
            }
            600
        }
        9 => {
            for index in 0..128usize {
                let low = ((index % 16) * 7) % 16;
                let high = ((index / 16) % 8) * 16;
                push_mode_width(&mut input, 3 + low + high);
            }
            128
        }
        _ => 0,
    };
    input.bytes[count_offset..count_offset + 2].copy_from_slice(&(count as u16).to_le_bytes());
    input
}

fn push_mode_width(input: &mut ModeInput, width: usize) {
    if width == 1 {
        push_mode(input, 0);
        return;
    }
    push_mode(input, 3);
    push_mode(input, (width - 2) as u8);
    for _ in 0..width - 2 {
        push_mode(input, 0);
    }
}

fn push_mode(input: &mut ModeInput, value: u8) {
    input.bytes[input.len] = value;
    input.len += 1;
}

fn push_mode_bytes(input: &mut ModeInput, value: &[u8]) {
    input.bytes[input.len..input.len + value.len()].copy_from_slice(value);
    input.len += value.len();
}

struct Input {
    bytes: [u8; 512],
    len: usize,
}

impl Input {
    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[inline(never)]
fn opaque_input(seed: u64, periodic: bool) -> Input {
    input(seed, periodic)
}

fn input(seed: u64, periodic: bool) -> Input {
    let mut input = Input {
        bytes: [0; 512],
        len: 0,
    };
    push(&mut input, 2);
    push_bytes(&mut input, &COUNT.to_le_bytes());
    for index in 0..COUNT {
        let class = mix(seed ^ u64::from(index)) % 10;
        if periodic {
            if (index / 20) % 2 == 0 {
                push(&mut input, 1);
                push(&mut input, (seed as u8).wrapping_add(index as u8) & 1);
            } else {
                push(&mut input, 0);
            }
        } else if class == 0 {
            push(&mut input, 2);
            push_bytes(&mut input, &2u16.to_le_bytes());
            push(&mut input, 0);
            push(&mut input, 1);
            push(&mut input, (seed as u8).wrapping_add(index as u8) & 1);
        } else if class.is_multiple_of(2) {
            push(&mut input, 0);
        } else {
            push(&mut input, 1);
            push(&mut input, (seed as u8).wrapping_add(index as u8) & 1);
        }
    }
    input
}

fn push(input: &mut Input, value: u8) {
    input.bytes[input.len] = value;
    input.len += 1;
}

fn push_bytes(input: &mut Input, value: &[u8]) {
    input.bytes[input.len..input.len + value.len()].copy_from_slice(value);
    input.len += value.len();
}

fn mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
