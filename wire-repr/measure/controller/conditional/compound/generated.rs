#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Foo {
    #[wire(as = u8)]
    present: bool,
    length: u8,
    #[wire(bytes = length)]
    first: wire_repr::wire::Bytes,
    #[wire(bytes = length)]
    second: wire_repr::wire::Bytes,
    #[wire(flag = present)]
    details: bool,
    #[wire(depends_on = details)]
    value: u8,
    #[wire(be, depends_on = details)]
    code: u16,
    tail: u8,
}

pub fn decode(seed: u64) -> u64 {
    if seed & 1 == 0 {
        decode_exact(present_input(seed)).unwrap_or(u64::MAX)
    } else {
        decode_exact(absent_input(seed)).unwrap_or(u64::MAX)
    }
}

fn decode_exact<const N: usize>(input: [u8; N]) -> Result<u64, ()> {
    let view = Foo::view(input).map_err(|_| ())?;
    let [first_0, first_1] = view.first() else {
        return Err(());
    };
    let [second_0, second_1] = view.second() else {
        return Err(());
    };
    let first = u16::from_le_bytes([*first_0, *first_1]);
    let second = u16::from_le_bytes([*second_0, *second_1]);
    Ok(u64::from(view.length())
        ^ u64::from(first).rotate_left(7)
        ^ u64::from(second).rotate_left(19)
        ^ u64::from(view.value().unwrap_or(0)).rotate_left(31)
        ^ u64::from(view.code().unwrap_or(0)).rotate_left(43)
        ^ u64::from(view.tail()).rotate_left(59))
}

pub fn build(seed: u64) -> u64 {
    match try_build(seed) {
        Ok(value) => value,
        Err(()) => u64::MAX,
    }
}

fn try_build(seed: u64) -> Result<u64, ()> {
    let first = (seed as u16).to_le_bytes();
    let second = ((seed >> 16) as u16).to_le_bytes();
    let mut output = [0u8; 10];
    let writer = discard(Foo::builder(&mut output[..]).first(&first[..]))?;
    let writer = discard(writer.second(&second[..]))?;
    let writer = if seed & 1 == 0 {
        discard(writer.details(|details| {
            details.present(|details| details.value((seed >> 32) as u8).code((seed >> 40) as u16))
        }))?
    } else {
        discard(writer.details(|details| details.absent()))?
    };
    let written = discard(discard(writer.tail((seed >> 56) as u8))?.finish())?;
    Ok(hash(written.as_bytes()))
}

#[inline(always)]
fn discard<T, E>(result: Result<T, E>) -> Result<T, ()> {
    result.map_err(|_| ())
}

fn present_input(seed: u64) -> [u8; 10] {
    let first = (seed as u16).to_le_bytes();
    let second = ((seed >> 16) as u16).to_le_bytes();
    let mut input = [0u8; 10];
    input[0] = 1;
    input[1] = 2;
    input[2..4].copy_from_slice(&first);
    input[4..6].copy_from_slice(&second);
    input[6] = (seed >> 32) as u8;
    input[7..9].copy_from_slice(&((seed >> 40) as u16).to_be_bytes());
    input[9] = (seed >> 56) as u8;
    input
}

fn absent_input(seed: u64) -> [u8; 7] {
    let first = (seed as u16).to_le_bytes();
    let second = ((seed >> 16) as u16).to_le_bytes();
    let mut input = [0u8; 7];
    input[0] = 0;
    input[1] = 2;
    input[2..4].copy_from_slice(&first);
    input[4..6].copy_from_slice(&second);
    input[6] = (seed >> 56) as u8;
    input
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
