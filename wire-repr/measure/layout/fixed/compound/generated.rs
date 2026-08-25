#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Bar {
    #[wire(le, constant = 0x1122)]
    tag: u16,
    value: u8,
}

#[derive(WireView, WireBuilder)]
struct Baz {
    #[wire(be, constant = 0x3344)]
    tag: u16,
    value: u8,
}

#[derive(WireView, WireBuilder)]
struct Foo<T, U, const N: usize> {
    header: u8,
    first: T,
    bytes: [u8; N],
    second: U,
    trailer: u8,
}

type Packet = Foo<Bar, Baz, 4>;

pub fn decode(seed: u64) -> u64 {
    let input = input(seed);
    let packet = Packet::view(input).unwrap();
    let first = packet.first();
    let second = packet.second();
    combine(
        packet.header(),
        first.value(),
        packet.bytes(),
        second.value(),
        packet.trailer(),
    )
}

pub fn build(seed: u64) -> u64 {
    match try_build(seed) {
        Ok(value) => value,
        Err(()) => u64::MAX,
    }
}

fn try_build(seed: u64) -> Result<u64, ()> {
    let mut output = [0u8; 12];
    let writer =
        discard(Packet::builder(&mut output[..]).second(|baz| baz.value((seed >> 24) as u8)))?;
    let writer = discard(writer.trailer((seed >> 32) as u8))?;
    let writer = discard(writer.bytes((seed as u32).to_le_bytes()))?;
    let writer = discard(writer.header((seed >> 40) as u8))?;
    let writer = discard(writer.first(|bar| bar.value((seed >> 16) as u8)))?;
    discard(writer.finish())?;
    Ok(hash(&output))
}

#[inline(always)]
fn discard<T, E>(result: Result<T, E>) -> Result<T, ()> {
    result.map_err(|_| ())
}

fn input(seed: u64) -> [u8; 12] {
    let mut input = [0u8; 12];
    input[0] = (seed >> 40) as u8;
    input[1..3].copy_from_slice(&0x1122u16.to_le_bytes());
    input[3] = (seed >> 16) as u8;
    input[4..8].copy_from_slice(&(seed as u32).to_le_bytes());
    input[8..10].copy_from_slice(&0x3344u16.to_be_bytes());
    input[10] = (seed >> 24) as u8;
    input[11] = (seed >> 32) as u8;
    input
}

fn combine(header: u8, first: u8, bytes: [u8; 4], second: u8, trailer: u8) -> u64 {
    u64::from(header)
        ^ u64::from(first).rotate_left(9)
        ^ u64::from(u32::from_le_bytes(bytes)).rotate_left(17)
        ^ u64::from(second).rotate_left(41)
        ^ u64::from(trailer).rotate_left(53)
}

fn hash(bytes: &[u8; 12]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
