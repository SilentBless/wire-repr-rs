#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

const ROOT_TAG: u32 = 0x1122_3344;
const LEAF_TAG: u32 = 0x5566_7788;

#[derive(WireView, WireBuilder)]
struct Foo<T, const TAG: u32> {
    #[wire(le, constant = TAG)]
    tag: u32,
    #[wire(le)]
    value: u32,
    child: T,
}

#[derive(WireView, WireBuilder)]
struct Bar<T> {
    #[wire(be)]
    value: i16,
    child: T,
}

#[derive(WireView, WireBuilder)]
struct Baz<T> {
    value: u8,
    child: T,
}

#[derive(WireView, WireBuilder)]
struct Qux {
    #[wire(le, constant = LEAF_TAG)]
    tag: u32,
    #[wire(as = u8)]
    enabled: bool,
}

type Packet = Foo<Bar<Baz<Qux>>, ROOT_TAG>;

pub fn decode(seed: u64) -> u64 {
    let foo = Packet::view(input(seed)).unwrap();
    let bar = foo.child();
    let baz = bar.child();
    let qux = baz.child();
    combine(
        foo.tag(),
        foo.value(),
        bar.value(),
        baz.value(),
        qux.tag(),
        qux.enabled(),
    )
}

pub fn build(seed: u64) -> u64 {
    match try_build(seed) {
        Ok(value) => value,
        Err(()) => u64::MAX,
    }
}

#[inline(always)]
fn try_build(seed: u64) -> Result<u64, ()> {
    let mut output = [0u8; 16];
    let writer = discard(Packet::builder(&mut output[..]).value(seed as u32))?;
    let writer = discard(writer.child(|bar| {
        bar.value((seed >> 32) as i16).child(|baz| {
            baz.value((seed >> 48) as u8)
                .child(|qux| qux.enabled(seed & 1 != 0))
        })
    }))?;
    discard(writer.finish())?;
    Ok(hash(&output))
}

#[inline(always)]
fn discard<T, E>(result: Result<T, E>) -> Result<T, ()> {
    result.map_err(|_| ())
}

fn input(seed: u64) -> [u8; 16] {
    let mut input = [0u8; 16];
    input[..4].copy_from_slice(&ROOT_TAG.to_le_bytes());
    input[4..8].copy_from_slice(&(seed as u32).to_le_bytes());
    input[8..10].copy_from_slice(&((seed >> 32) as i16).to_be_bytes());
    input[10] = (seed >> 48) as u8;
    input[11..15].copy_from_slice(&LEAF_TAG.to_le_bytes());
    input[15] = u8::from(seed & 1 != 0);
    input
}

fn combine(tag: u32, value: u32, nested: i16, byte: u8, leaf: u32, enabled: bool) -> u64 {
    u64::from(tag)
        ^ u64::from(value).rotate_left(7)
        ^ u64::from(nested as u16).rotate_left(19)
        ^ u64::from(byte).rotate_left(31)
        ^ u64::from(leaf).rotate_left(41)
        ^ u64::from(enabled)
}

fn hash(bytes: &[u8; 16]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}
