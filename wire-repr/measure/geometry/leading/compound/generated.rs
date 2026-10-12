#![allow(dead_code)]

#[path = "fixture.rs"]
mod fixture;

use fixture::CAPACITY;
use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Part {
    length: u8,
    #[wire(bytes = length)]
    payload: wire_repr::wire::Bytes,
}

#[derive(WireView, WireBuilder)]
struct Record<T> {
    first: T,
    #[wire(be)]
    middle: u16,
    second: T,
    #[wire(le)]
    tail: u32,
}

#[derive(WireBuilder)]
struct Envelope {
    body: Record<Part>,
}

pub fn decode(seed: u64) -> u64 {
    let input = fixture::input(seed);
    decode_input(input.as_slice()).unwrap_or(u64::MAX)
}

#[inline(always)]
fn decode_input(input: &[u8]) -> Option<u64> {
    let view = Record::<Part>::view(input).ok()?;
    Some(fixture::combine(
        view.first().payload(),
        view.middle(),
        view.second().payload(),
        view.tail(),
    ))
}

pub fn build(seed: u64) -> u64 {
    try_build(seed).unwrap_or(u64::MAX)
}

fn try_build(seed: u64) -> Option<u64> {
    let values = fixture::values(seed);
    let mut output = [0u8; CAPACITY];
    let writer = Record::<Part>::builder(&mut output[..])
        .first(|child| child.payload(&values.first[..values.first_len]))
        .ok()?
        .middle(values.middle)
        .ok()?
        .second(|child| child.payload(&values.second[..values.second_len]))
        .ok()?
        .tail(values.tail)
        .ok()?;
    let written = writer.finish().ok()?;
    Some(fixture::hash(written.as_bytes()))
}

pub fn detached_build(seed: u64) -> u64 {
    try_detached_build(seed).unwrap_or(u64::MAX)
}

fn try_detached_build(seed: u64) -> Option<u64> {
    let values = fixture::values(seed);
    let mut output = [0u8; CAPACITY];
    let written = Envelope::builder(&mut output[..])
        .body(|body| {
            body.first(|child| child.payload(&values.first[..values.first_len]))
                .middle(values.middle)
                .second(|child| child.payload(&values.second[..values.second_len]))
                .tail(values.tail)
        })
        .ok()?
        .finish()
        .ok()?;
    Some(fixture::hash(core::hint::black_box(written.as_bytes())))
}

pub fn copy(seed: u64) -> u64 {
    try_copy(seed).unwrap_or(u64::MAX)
}

fn try_copy(seed: u64) -> Result<u64, ()> {
    let input = fixture::input(seed);
    let source = Record::<Part>::view(input.as_slice()).map_err(|_| ())?;
    let bytes = source.as_bytes();
    let mut output = [0u8; CAPACITY];
    output[..bytes.len()].copy_from_slice(bytes);
    Ok(fixture::hash(&output[..bytes.len()]))
}

pub fn domain(seed: u64) -> u64 {
    let input = fixture::domain_input(seed);
    decode_input(input.as_slice()).unwrap_or(u64::MAX)
}
