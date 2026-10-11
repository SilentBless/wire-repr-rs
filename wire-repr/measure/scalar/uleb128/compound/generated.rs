#![allow(dead_code)]

#[path = "fixture.rs"]
mod fixture;

use fixture::{CAPACITY, Values};
use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct UnsignedRecord {
    #[wire(uleb128)]
    byte: u8,
    #[wire(uleb128)]
    short: u16,
    #[wire(uleb128)]
    word: u32,
    #[wire(uleb128)]
    long: u64,
    #[wire(uleb128)]
    wide: u128,
    #[wire(uleb128)]
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
    let Ok(view) = UnsignedRecord::view(&input[..length]) else {
        return u64::MAX;
    };
    fixture::observe(
        Values {
            byte: view.byte(),
            short: view.short(),
            word: view.word(),
            long: view.long(),
            wide: view.wide(),
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
    let writer = discard(UnsignedRecord::builder(&mut output[..]).byte(values.byte))?;
    let writer = discard(writer.short(values.short))?;
    let writer = discard(writer.word(values.word))?;
    let writer = discard(writer.long(values.long))?;
    let writer = discard(writer.wide(values.wide))?;
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
    let source = UnsignedRecord::view(&input[..length]).map_err(|_| ())?;
    let mut output = [0u8; CAPACITY + 1];
    let writer = discard(
        RecordHolder::<UnsignedRecord>::builder(&mut output[..])
            .records(|records| records.item_view(source)),
    )?;
    let written = discard(writer.finish())?;
    Ok(fixture::hash(written.as_bytes()))
}

#[inline(always)]
fn discard<T, E>(result: Result<T, E>) -> Result<T, ()> {
    result.map_err(|_| ())
}
