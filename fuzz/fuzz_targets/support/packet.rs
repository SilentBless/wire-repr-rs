#![allow(dead_code)]

use std::fmt;

use wire_repr::{ByteSelection, WireBuilder, WireView, select};

fn checksum16(selection: impl ByteSelection) -> u16 {
    selection
        .bytes()
        .fold(0u16, |sum, byte| sum.wrapping_add(u16::from(byte)))
}

#[derive(Clone, Copy, Debug)]
struct ChecksumMismatch;

impl fmt::Display for ChecksumMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("stored checksum does not match represented bytes")
    }
}

impl std::error::Error for ChecksumMismatch {}

#[wire_repr::validator]
fn validate_packet(view: &impl PacketView) -> Result<(), ChecksumMismatch> {
    let expected = checksum16(select(view).exclude(|fields| fields.checksum));
    (view.checksum() == expected)
        .then_some(())
        .ok_or(ChecksumMismatch)
}

#[derive(WireView, WireBuilder)]
pub struct Item {
    #[wire(be)]
    value: u16,
}

#[derive(WireView, WireBuilder)]
#[wire(validate = validate_packet)]
pub struct Packet {
    #[wire(le, computed = checksum16(exclude(self)))]
    checksum: u16,
    body_len: u8,
    count: u8,
    #[wire(as = u8)]
    present: bool,
    #[wire(flag = present)]
    details: bool,
    #[wire(be, depends_on = details)]
    extra: u16,
    #[wire(bytes = body_len)]
    body: wire_repr::wire::Bytes,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<Item>,
    tail: u8,
}

pub fn inspect_packet(input: &[u8]) {
    let Ok(view) = Packet::view(input) else {
        return;
    };
    assert_eq!(view.as_bytes(), input);
    assert_eq!(view.body().len(), usize::from(view.body_len()));
    assert_eq!(view.items().len(), usize::from(view.count()));
    assert_eq!(view.present(), view.details());
    assert_eq!(view.extra().is_some(), view.details());
    let mut cursor = 5usize;
    if view.details() {
        cursor += 2;
    }
    let body_end = cursor + usize::from(view.body_len());
    assert_eq!(view.body(), &input[cursor..body_end]);
    cursor = body_end;
    let items_end = cursor + usize::from(view.count()) * 2;
    let expected_items = input[cursor..items_end]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| u16::from_be_bytes(*bytes))
        .collect::<Vec<_>>();
    assert_eq!(input[items_end], view.tail());
    assert_eq!(items_end + 1, input.len());

    let physical_checksum = input[2..]
        .iter()
        .fold(0u16, |sum, byte| sum.wrapping_add(u16::from(*byte)));
    assert_eq!(view.checksum(), physical_checksum);

    let expected = checksum16(select(&view).exclude(|fields| fields.checksum));
    assert_eq!(view.checksum(), expected);

    let sequential = view
        .items()
        .iter()
        .map(|item| item.map(|item| item.view().value()))
        .collect::<Result<Vec<_>, _>>()
        .expect("framed array items");
    assert_eq!(sequential.len(), view.items().len());
    let replayed = view
        .items()
        .iter()
        .map(|item| item.map(|item| item.view().value()))
        .collect::<Result<Vec<_>, _>>()
        .expect("replayed array items");
    assert_eq!(sequential, replayed);
    assert_eq!(sequential, expected_items);
}

pub fn roundtrip_packet(data: &[u8]) {
    if data.len() < 4 {
        return;
    }
    let present = data[0] & 1 != 0;
    let extra = u16::from_le_bytes([data[1], data[2]]);
    let tail = data[3];
    let body_len = data.get(4).copied().unwrap_or(0) as usize % 33;
    let body_end = 5usize.saturating_add(body_len).min(data.len());
    let body = data.get(5..body_end).unwrap_or_default();
    let values = data[body_end..]
        .as_chunks::<2>()
        .0
        .iter()
        .take(16)
        .map(|bytes| u16::from_le_bytes(*bytes))
        .collect::<Vec<_>>();

    macro_rules! build_packet {
        ($output:expr) => {{
            let writer = Packet::builder($output)
                .details(|details| {
                    if present {
                        details.present(|details| details.extra(extra))
                    } else {
                        details.absent()
                    }
                })
                .expect("conditional group is representable")
                .body(body)
                .expect("bounded body is representable")
                .items(|items| {
                    items.try_extend(values.iter().copied(), |item, value| item.value(value))
                })
                .expect("bounded item count is representable")
                .tail(tail)
                .expect("tail is representable");
            writer.finish().expect("complete packet finishes")
        }};
    }

    let mut fixed = [0u8; 80];
    let fixed_bytes = build_packet!(&mut fixed[..]).as_bytes().to_vec();

    let owned = build_packet!(wire_repr::output::owned(Vec::new()))
        .as_bytes()
        .to_vec();
    assert_eq!(fixed_bytes, owned);

    let mut expected = vec![
        0,
        0,
        u8::try_from(body.len()).expect("bounded body length"),
        u8::try_from(values.len()).expect("bounded item count"),
        u8::from(present),
    ];
    if present {
        expected.extend_from_slice(&extra.to_be_bytes());
    }
    expected.extend_from_slice(body);
    for value in &values {
        expected.extend_from_slice(&value.to_be_bytes());
    }
    expected.push(tail);
    let checksum = expected[2..]
        .iter()
        .fold(0u16, |sum, byte| sum.wrapping_add(u16::from(*byte)));
    expected[..2].copy_from_slice(&checksum.to_le_bytes());
    assert_eq!(fixed_bytes, expected);

    let view = Packet::view(&fixed_bytes).expect("generated representation frames");
    assert_eq!(view.body(), body);
    assert_eq!(view.details(), present);
    assert_eq!(view.extra(), present.then_some(extra));
    assert_eq!(view.tail(), tail);
    let observed = view
        .items()
        .iter()
        .map(|item| item.map(|item| item.view().value()))
        .collect::<Result<Vec<_>, _>>()
        .expect("generated items frame");
    assert_eq!(observed, values);
    inspect_packet(&fixed_bytes);
}
