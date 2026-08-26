#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Foo {
    offset: u8,
    length: u8,
    head: u8,
    #[wire(at = offset, bytes = length)]
    body: wire_repr::wire::Bytes,
    #[wire(be, align_before = 4)]
    tail: u16,
    #[wire(rest)]
    rest: wire_repr::wire::Bytes,
}

pub fn decode(seed: u64) -> u64 {
    let input = input(seed);
    let view = Foo::view(input).unwrap();
    combine(view.head(), view.body(), view.tail(), view.rest())
}

pub fn build(seed: u64) -> u64 {
    match try_build(seed) {
        Ok(value) => value,
        Err(()) => u64::MAX,
    }
}

fn try_build(seed: u64) -> Result<u64, ()> {
    let body = body(seed);
    let rest = rest(seed);
    let mut output = [0u8; 18];
    let writer = discard(Foo::builder(&mut output[..]).offset(6))?;
    let writer = discard(writer.head((seed >> 48) as u8))?;
    let writer = discard(writer.body(&body[..]))?;
    let writer = discard(writer.tail((seed >> 24) as u16))?;
    let writer = discard(writer.rest(&rest[..]))?;
    discard(writer.finish())?;
    Ok(hash(&output))
}

#[inline(always)]
fn discard<T, E>(result: Result<T, E>) -> Result<T, ()> {
    result.map_err(|_| ())
}

fn body(seed: u64) -> [u8; 3] {
    [seed as u8, (seed >> 8) as u8, (seed >> 16) as u8]
}

fn rest(seed: u64) -> [u8; 4] {
    (seed as u32).rotate_left(11).to_le_bytes()
}

fn input(seed: u64) -> [u8; 18] {
    let mut input = [0u8; 18];
    input[0] = 6;
    input[1] = 3;
    input[2] = (seed >> 48) as u8;
    input[6..9].copy_from_slice(&body(seed));
    input[12..14].copy_from_slice(&((seed >> 24) as u16).to_be_bytes());
    input[14..18].copy_from_slice(&rest(seed));
    input
}

fn combine(head: u8, body: &[u8], tail: u16, rest: &[u8]) -> u64 {
    u64::from(head)
        ^ u64::from(u32::from_le_bytes([body[0], body[1], body[2], 0])).rotate_left(11)
        ^ u64::from(tail).rotate_left(31)
        ^ u64::from(u32::from_le_bytes(rest.try_into().unwrap())).rotate_left(47)
}

fn hash(bytes: &[u8; 18]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
