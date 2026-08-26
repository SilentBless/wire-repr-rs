#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Bar {
    length: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
}

#[derive(WireView, WireBuilder)]
struct Foo<T> {
    count: u8,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<T>,
    tail: u8,
}

pub fn decode(seed: u64) -> u64 {
    try_decode(seed).unwrap_or(u64::MAX)
}

fn try_decode(seed: u64) -> Result<u64, ()> {
    let input = opaque_input(seed);
    decode_input(input.as_slice())
}

fn decode_input(input: &[u8]) -> Result<u64, ()> {
    let view = Foo::<Bar>::view(input).map_err(|_| ())?;
    let mut value = u64::from(view.tail()).rotate_left(53);
    for item in view.items().iter() {
        let item = item.map_err(|_| ())?;
        for byte in item.view().body() {
            value = value.rotate_left(7) ^ u64::from(*byte);
        }
    }
    Ok(value)
}

pub fn build(seed: u64) -> u64 {
    match try_build(seed) {
        Ok(value) => value,
        Err(()) => u64::MAX,
    }
}

fn try_build(seed: u64) -> Result<u64, ()> {
    let first = [seed as u8];
    let second = [(seed >> 8) as u8, (seed >> 16) as u8];
    let third = [(seed >> 24) as u8];
    let mut output = [0u8; 9];
    let writer = discard(Foo::<Bar>::builder(&mut output[..]).items(|items| {
        let items = items.item(|bar| bar.body(&first[..]))?;
        let items = items.item(|bar| bar.body(&second[..]))?;
        let items = items.item(|bar| bar.body(&third[..]))?;
        Ok(items)
    }))?;
    let written = discard(discard(writer.tail((seed >> 32) as u8))?.finish())?;
    Ok(hash(written.as_bytes()))
}

pub fn copy(seed: u64) -> u64 {
    match try_copy(seed) {
        Ok(value) => value,
        Err(()) => u64::MAX,
    }
}

fn try_copy(seed: u64) -> Result<u64, ()> {
    let input = opaque_input(seed);
    copy_input(input.as_slice())
}

fn copy_input(input: &[u8]) -> Result<u64, ()> {
    let source = Foo::<Bar>::view(input).map_err(|_| ())?;
    let mut output = [0u8; 16];
    let writer = discard(
        Foo::<Bar>::builder(&mut output[..]).items(|items| items.copy_from(source.items())),
    )?;
    let written = discard(discard(writer.tail(source.tail().wrapping_add(1)))?.finish())?;
    Ok(observe(written.as_bytes()))
}

#[inline(always)]
fn discard<T, E>(result: Result<T, E>) -> Result<T, ()> {
    result.map_err(|_| ())
}

pub fn domain(seed: u64) -> u64 {
    let input = domain_input(seed);
    let decode = decode_input(input.as_slice()).unwrap_or(u64::MAX);
    let copy = copy_input(input.as_slice()).unwrap_or(u64::MAX);
    decode.rotate_left(17) ^ copy
}

struct Input {
    bytes: [u8; 16],
    len: usize,
}

impl Input {
    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[inline(never)]
fn opaque_input(seed: u64) -> Input {
    input(seed)
}

fn input(seed: u64) -> Input {
    let mut bytes = [0u8; 16];
    bytes[0] = 3;
    let mut cursor = 1usize;
    for index in 0..3usize {
        let length = ((seed >> (index * 8)) as usize % 3) + 1;
        bytes[cursor] = length as u8;
        cursor += 1;
        for item in 0..length {
            bytes[cursor] = seed.rotate_right((index * 11 + item * 7) as u32) as u8;
            cursor += 1;
        }
    }
    bytes[cursor] = (seed >> 32) as u8;
    Input {
        bytes,
        len: cursor + 1,
    }
}

fn domain_input(seed: u64) -> Input {
    let mut input = input(seed);
    match seed & 7 {
        0 => input.bytes[0] = 0,
        1 => input.bytes[0] = 4,
        2 => input.bytes[1] = 15,
        3 => input.len -= 1,
        _ => {}
    }
    input
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}

#[inline(never)]
fn observe(bytes: &[u8]) -> u64 {
    hash(bytes)
}
