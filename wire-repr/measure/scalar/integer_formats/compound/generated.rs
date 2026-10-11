#![allow(dead_code)]

#[path = "fixture.rs"]
mod fixture;

use fixture::{CAPACITY, Values};
use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct IntegerRecord {
    #[wire(sleb128)]
    signed: i64,
    #[wire(uleb128, zigzag)]
    transformed: i32,
    #[wire(vu128)]
    prefix: u64,
    #[wire(vu128, zigzag)]
    signed_prefix: i128,
    #[wire(vu128)]
    payload_bytes: u16,
    #[wire(bytes = payload_bytes)]
    payload: wire_repr::wire::Bytes,
    #[wire(le)]
    tail: u16,
}

#[derive(WireBuilder)]
struct RecordHolder<T> {
    count: u8,
    #[wire(counted_by = count)]
    records: wire_repr::wire::Array<T>,
}

pub fn decode(seed: u64) -> u64 {
    let (input, length) = fixture::source(seed);
    let Ok(view) = IntegerRecord::view(&input[..length]) else {
        return u64::MAX;
    };
    fixture::observe(
        Values {
            signed: view.signed(),
            transformed: view.transformed(),
            prefix: view.prefix(),
            signed_prefix: view.signed_prefix(),
        },
        view.payload(),
        view.tail(),
    )
}

pub fn build(seed: u64) -> u64 {
    try_build(seed).unwrap_or(u64::MAX)
}

fn try_build(seed: u64) -> Result<u64, ()> {
    let values = fixture::values(seed);
    let (payload, length) = fixture::payload(seed);
    let mut output = [0u8; CAPACITY];
    let writer = discard(IntegerRecord::builder(&mut output[..]).signed(values.signed))?;
    let writer = discard(writer.transformed(values.transformed))?;
    let writer = discard(writer.prefix(values.prefix))?;
    let writer = discard(writer.signed_prefix(values.signed_prefix))?;
    let writer = discard(writer.payload(&payload[..length]))?;
    let writer = discard(writer.tail(fixture::tail(seed)))?;
    let written = discard(writer.finish())?;
    Ok(fixture::hash(written.as_bytes()))
}

pub fn copy(seed: u64) -> u64 {
    try_copy(seed).unwrap_or(u64::MAX)
}

fn try_copy(seed: u64) -> Result<u64, ()> {
    let (input, length) = fixture::source(seed);
    let source = IntegerRecord::view(&input[..length]).map_err(|_| ())?;
    let mut output = [0u8; CAPACITY + 1];
    let writer = discard(
        RecordHolder::<IntegerRecord>::builder(&mut output[..])
            .records(|records| records.item_view(source)),
    )?;
    let written = discard(writer.finish())?;
    Ok(fixture::hash(written.as_bytes()))
}

#[inline(always)]
fn discard<T, E>(result: Result<T, E>) -> Result<T, ()> {
    result.map_err(|_| ())
}
