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

#[derive(Clone, Copy)]
struct UserId(u32);

impl TryFrom<u32> for UserId {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        (value != 0).then_some(Self(value)).ok_or(())
    }
}

impl From<UserId> for u32 {
    fn from(value: UserId) -> Self {
        value.0
    }
}

#[derive(WireView, WireBuilder)]
struct RustTypes {
    #[wire(as = u32, le)]
    id: UserId,
    #[wire(as = u16, be)]
    count: core::num::NonZeroU16,
}

pub fn rust_decode(seed: u64) -> u64 {
    let bytes = seed.to_le_bytes();
    let input: [u8; 6] = bytes[..6].try_into().unwrap();
    match RustTypes::view(input) {
        Ok(value) => {
            u64::from(u32::from(value.id())) ^ u64::from(value.count().get()).rotate_left(17)
        }
        Err(_) => u64::MAX,
    }
}

pub fn rust_build(seed: u64) -> u64 {
    let Some(id) = UserId::try_from(seed as u32).ok() else {
        return u64::MAX;
    };
    let Some(count) = core::num::NonZeroU16::new((seed >> 32) as u16) else {
        return u64::MAX;
    };
    let mut output = [0u8; 6];
    let Ok(complete) = RustTypes::builder(&mut output[..]).id(id) else {
        return u64::MAX;
    };
    let Ok(complete) = complete.count(count) else {
        return u64::MAX;
    };
    if complete.finish().is_err() {
        return u64::MAX;
    }
    let mut padded = [0u8; 8];
    padded[..6].copy_from_slice(&output);
    u64::from_le_bytes(padded)
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
