#![allow(dead_code)]

use wire_repr::{WireView, select};

#[derive(WireView)]
struct Foo {
    first: u8,
    length: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
    tail: u8,
}

pub fn bytes(seed: u64) -> u64 {
    let input = input(seed);
    let Ok(view) = Foo::view(input) else {
        return u64::MAX;
    };
    select(&view)
        .include(|fields| fields.tail | fields.first | fields.body)
        .bytes()
        .fold(0u64, |value, byte| value.rotate_left(5) ^ u64::from(byte))
}

pub fn chunks(seed: u64) -> u64 {
    let input = input(seed);
    let Ok(view) = Foo::view(input) else {
        return u64::MAX;
    };
    select(&view)
        .exclude(|fields| fields.body)
        .chunks()
        .fold(0u64, |value, chunk| {
            chunk
                .iter()
                .fold(value ^ chunk.len() as u64, |value, byte| {
                    value.rotate_left(5) ^ u64::from(*byte)
                })
        })
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
