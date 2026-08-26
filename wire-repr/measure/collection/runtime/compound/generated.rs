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
    let source = Foo::<Bar>::view(opaque_input(seed)).map_err(|_| ())?;
    let mut output = [0u8; 9];
    let writer = discard(Foo::<Bar>::builder(&mut output[..]).items(|mut items| {
        for item in source.items().iter() {
            items = items.item_result(item)?;
        }
        Ok(items)
    }))?;
    let written = discard(discard(writer.tail(source.tail()))?.finish())?;
    Ok(hash(written.as_bytes()))
}

#[inline(always)]
fn discard<T, E>(result: Result<T, E>) -> Result<T, ()> {
    result.map_err(|_| ())
}

#[inline(never)]
fn opaque_input(seed: u64) -> [u8; 9] {
    input(seed)
}

fn input(seed: u64) -> [u8; 9] {
    [
        3,
        1,
        seed as u8,
        2,
        (seed >> 8) as u8,
        (seed >> 16) as u8,
        1,
        (seed >> 24) as u8,
        (seed >> 32) as u8,
    ]
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
