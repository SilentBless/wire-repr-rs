#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
#[wire(as = u16, be)]
struct Foo {
    #[wire(bit = 0)]
    enabled: bool,
    #[wire(bits = 1..=3)]
    kind: u8,
    #[wire(bits = 8..=11)]
    code: u8,
}

#[derive(WireView, WireBuilder)]
struct Bar {
    #[wire(le)]
    raw: u16,
    #[wire(bits_of = raw, bit = 0)]
    enabled: bool,
    #[wire(bits_of = raw, bits = 1..=3)]
    kind: u8,
    tail: u8,
}

pub fn decode(seed: u64) -> u64 {
    try_decode(seed).unwrap_or(u64::MAX)
}

fn try_decode(seed: u64) -> Result<u64, ()> {
    let raw = seed as u16;
    let foo = Foo::view(raw.to_be_bytes()).map_err(|_| ())?;
    let bar = Bar::view([raw as u8, (raw >> 8) as u8, (seed >> 16) as u8]).map_err(|_| ())?;
    Ok(u64::from(foo.enabled())
        | (u64::from(foo.kind()) << 1)
        | (u64::from(foo.code()) << 8)
        | (u64::from(bar.enabled()) << 16)
        | (u64::from(bar.kind()) << 17)
        | (u64::from(bar.tail()) << 24))
}

pub fn build(seed: u64) -> u64 {
    try_build(seed).unwrap_or(u64::MAX)
}

fn try_build(seed: u64) -> Result<u64, ()> {
    let mut output = [0u8; 5];
    discard(
        Foo::builder(&mut output[..2])
            .enabled(seed & 1 != 0)
            .map_err(|_| ())?
            .kind(((seed >> 1) & 7) as u8)
            .map_err(|_| ())?
            .code(((seed >> 8) & 15) as u8)
            .map_err(|_| ())?
            .finish(),
    )?;
    discard(
        Bar::builder(&mut output[2..])
            .enabled((seed >> 16) & 1 != 0)
            .map_err(|_| ())?
            .kind(((seed >> 17) & 7) as u8)
            .map_err(|_| ())?
            .tail((seed >> 24) as u8)
            .map_err(|_| ())?
            .finish(),
    )?;
    Ok(hash(&output))
}

#[inline(always)]
fn discard<T, E>(result: Result<T, E>) -> Result<T, ()> {
    result.map_err(|_| ())
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
