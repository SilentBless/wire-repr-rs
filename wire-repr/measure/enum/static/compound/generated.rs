#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Bar {
    #[wire(le)]
    value: u32,
}

#[derive(WireView, WireBuilder)]
struct Baz {
    bytes: [u8; 4],
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u16, le)]
enum Foo {
    #[wire(value = 1)]
    First(Bar),
    #[wire(value = 2)]
    Second(Baz),
    #[wire(unknown)]
    Unknown(wire_repr::wire::Bytes),
}

pub fn decode(seed: u64) -> u64 {
    try_decode(seed).unwrap_or(u64::MAX)
}

fn try_decode(seed: u64) -> Result<u64, ()> {
    let input = input(seed);
    let view = Foo::view(input).map_err(|_| ())?;
    Ok(match view.variant() {
        FooVariant::First(value) => u64::from(value.value()).rotate_left(7),
        FooVariant::Second(value) => {
            let bytes = value.bytes();
            u64::from(bytes[0]) | (u64::from(bytes[1]) << 8)
        }
        FooVariant::Unknown { selector, body } => {
            body.iter().fold(u64::from(selector), |value, byte| {
                value.rotate_left(5) ^ u64::from(*byte)
            })
        }
    })
}

pub fn build(seed: u64) -> u64 {
    try_build(seed).unwrap_or(u64::MAX)
}

fn try_build(seed: u64) -> Result<u64, ()> {
    let mut output = [0u8; 6];
    let length = match seed % 3 {
        0 => discard(Foo::builder(&mut output[..]).first(|bar| bar.value(seed as u32)))?
            .finish()
            .map_err(|_| ())?
            .len(),
        1 => discard(
            Foo::builder(&mut output[..])
                .second(|baz| baz.bytes([seed as u8, (seed >> 8) as u8, 0, 0])),
        )?
        .finish()
        .map_err(|_| ())?
        .len(),
        _ => discard(Foo::builder(&mut output[..]).unknown(
            7,
            &[seed as u8, (seed >> 8) as u8, (seed >> 16) as u8, 0][..],
        ))?
        .finish()
        .map_err(|_| ())?
        .len(),
    };
    Ok(hash(&output[..length]))
}

pub fn copy(seed: u64) -> u64 {
    try_copy(seed).unwrap_or(u64::MAX)
}

fn try_copy(seed: u64) -> Result<u64, ()> {
    #[derive(WireBuilder)]
    struct Envelope<T> {
        count: u8,
        #[wire(counted_by = count)]
        values: wire_repr::wire::Array<T>,
    }
    let source = Foo::view(input(seed)).map_err(|_| ())?;
    let mut output = [0u8; 7];
    let written = discard(
        discard(
            Envelope::<Foo>::builder(&mut output[..]).values(|values| values.item_view(source)),
        )?
        .finish(),
    )?;
    Ok(hash(written.as_bytes()))
}

#[inline(always)]
fn discard<T, E>(result: Result<T, E>) -> Result<T, ()> {
    result.map_err(|_| ())
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
