#![allow(dead_code)]

use core::convert::Infallible;

use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
pub(crate) struct Foo {
    #[wire(le)]
    foo: u32,
    #[wire(be)]
    bar: u32,
}

#[derive(WireView, WireBuilder)]
struct Bar {
    #[wire(le, constant = 0x4433_2211)]
    foo: u32,
}

#[derive(WireView, WireBuilder)]
struct Arrays {
    #[wire(le)]
    values: [u16; 4],
}

pub fn array_decode(seed: u64) -> u64 {
    let Ok(view) = Arrays::view(seed.to_le_bytes()) else {
        return u64::MAX;
    };
    view.values()
        .into_iter()
        .fold(0u64, |hash, value| hash.rotate_left(11) ^ u64::from(value))
}

pub fn array_build(seed: u64) -> u64 {
    let bytes = seed.to_le_bytes();
    let values =
        core::array::from_fn(|index| u16::from_le_bytes([bytes[index * 2], bytes[index * 2 + 1]]));
    let mut output = [0u8; 8];
    let Ok(complete) = Arrays::builder(&mut output[..]).values(values) else {
        return u64::MAX;
    };
    if complete.finish().is_err() {
        return u64::MAX;
    }
    u64::from_le_bytes(output)
}

type WriteFailure = wire_repr::WriteError<Infallible, Infallible>;

pub fn decode(seed: u64) -> u64 {
    let input = seed.to_le_bytes();
    let foo = Foo::view(input).unwrap();
    u64::from(foo.foo()) ^ u64::from(foo.bar()).rotate_left(17)
}

pub fn build(seed: u64) -> u64 {
    match try_build(seed) {
        Ok(value) => value,
        Err(_) => u64::MAX,
    }
}

#[inline(always)]
fn try_build(seed: u64) -> Result<u64, WriteFailure> {
    let mut output = [0u8; 8];
    Foo::builder(&mut output[..])
        .foo(seed as u32)?
        .bar((seed >> 32) as u32)?
        .finish()?;
    Ok(u64::from_le_bytes(output))
}

pub fn constant_decode(seed: u64) -> u64 {
    u64::from(Bar::view((seed as u32).to_le_bytes()).is_ok())
}

pub fn constant_build(_seed: u64) -> u64 {
    match try_constant_build() {
        Ok(value) => value,
        Err(_) => u64::MAX,
    }
}

#[inline(always)]
fn try_constant_build() -> Result<u64, WriteFailure> {
    let mut output = [0u8; 4];
    Bar::builder(&mut output[..]).finish()?;
    Ok(u64::from(u32::from_le_bytes(output)))
}
