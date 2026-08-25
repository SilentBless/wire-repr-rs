#![allow(dead_code)]

use core::convert::Infallible;

use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Bar {
    #[wire(le, constant = 0x4433_2211)]
    foo: u32,
}

#[derive(WireView, WireBuilder)]
struct Foo<T> {
    #[wire(le)]
    foo: u32,
    bar: T,
}

type FooBar = Foo<Bar>;
type WriteFailure = wire_repr::WriteError<FooWriteError<Infallible>, Infallible>;

pub fn decode(seed: u64) -> u64 {
    match FooBar::view(seed.to_le_bytes()) {
        Ok(foo) => u64::from(foo.foo()) + u64::from(foo.bar().foo()),
        Err(_) => u64::MAX,
    }
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
    FooBar::builder(&mut output[..])
        .foo(seed as u32)?
        .bar(|bar| bar)?
        .finish()?;
    Ok(u64::from_le_bytes(output))
}
