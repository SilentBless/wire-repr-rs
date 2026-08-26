#![allow(dead_code)]

use wire_repr::{ByteSelection, WireBuilder, WireView};

fn sum(selection: impl ByteSelection) -> u16 {
    selection.bytes().map(u16::from).sum()
}

fn doubled(value: u16) -> u16 {
    value * 2
}

#[derive(WireView, WireBuilder)]
struct Foo {
    first: u8,
    #[wire(le, computed = doubled(second))]
    third: u16,
    #[wire(le, computed = sum(include(first, tail)))]
    second: u16,
    tail: u8,
}

#[derive(WireView, WireBuilder)]
struct Dynamic {
    #[wire(le, computed = sum(exclude(self)))]
    checksum: u16,
    length: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
}

pub fn fixed(seed: u64) -> u64 {
    let mut output = [0u8; 6];
    let result = Foo::builder(&mut output[..])
        .first(seed as u8)
        .and_then(|writer| writer.tail((seed >> 8) as u8))
        .and_then(|writer| writer.finish());
    match result {
        Ok(written) => hash(written.as_bytes()),
        Err(_) => u64::MAX,
    }
}

pub fn dynamic(seed: u64) -> u64 {
    let body = [seed as u8, (seed >> 8) as u8, (seed >> 16) as u8];
    let mut output = [0u8; 6];
    let result = Dynamic::builder(&mut output[..])
        .body(&body[..])
        .and_then(|writer| writer.finish());
    match result {
        Ok(written) => hash(written.as_bytes()),
        Err(_) => u64::MAX,
    }
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
