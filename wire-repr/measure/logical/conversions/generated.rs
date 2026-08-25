#![allow(dead_code)]

use core::convert::Infallible;

use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Foo {
    #[wire(as = u16, le)]
    foo: usize,
    #[wire(as = u8)]
    bar: bool,
    #[wire(as = u32, be)]
    baz: char,
}

type WriteFailure = wire_repr::WriteError<FooWriteError, Infallible>;

pub fn decode(seed: u64) -> u64 {
    let bytes = seed.to_le_bytes();
    let input: [u8; 7] = bytes[..7].try_into().unwrap();
    match Foo::view(input) {
        Ok(foo) => {
            u64::try_from(foo.foo()).unwrap()
                ^ u64::from(foo.bar())
                ^ u64::from(u32::from(foo.baz())).rotate_left(23)
        }
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
    let mut output = [0u8; 7];
    Foo::builder(&mut output[..])
        .foo(seed as usize)?
        .bar(seed & 1 != 0)?
        .baz('λ')?
        .finish()?;
    let mut padded = [0u8; 8];
    padded[..7].copy_from_slice(&output);
    Ok(u64::from_le_bytes(padded))
}
