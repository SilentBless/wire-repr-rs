use wire_repr::{ArrayError, NeedMore, TrailingBytes, WireBuilder, WireView, wire};

#[derive(WireView, WireBuilder)]
struct Attribute {
    length: u8,
    #[wire(bytes = length)]
    data: wire::Bytes,
}

#[derive(WireView, WireBuilder)]
struct Method {
    attribute_count: u8,
    #[wire(counted_by = attribute_count)]
    attributes: wire::Array<Attribute>,
}

#[derive(WireView, WireBuilder)]
struct Packet {
    method_count: u8,
    #[wire(counted_by = method_count)]
    methods: wire::Array<Method>,
    tail: u8,
}

#[derive(WireView, WireBuilder)]
pub struct Entry {
    #[wire(be)]
    key: i32,
    #[wire(be)]
    value: i32,
}

#[derive(WireView, WireBuilder)]
pub struct Batch {
    #[wire(be)]
    count: i32,
    #[wire(counted_by = count)]
    entries: wire::Array<Entry>,
    #[wire(as = u8)]
    enabled: bool,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u16, be)]
pub enum Envelope {
    #[wire(value = 0x3102)]
    Batch(Batch),
}

#[derive(WireView, WireBuilder)]
struct CountItem {
    value: u8,
}

#[derive(WireView, WireBuilder)]
struct SignedWidths {
    #[wire(be)]
    short_count: i16,
    #[wire(counted_by = short_count)]
    short_items: wire::Array<CountItem>,
    #[wire(le)]
    long_count: i64,
    #[wire(counted_by = long_count)]
    long_items: wire::Array<CountItem>,
    #[wire(be)]
    wide_count: i128,
    #[wire(counted_by = wide_count)]
    wide_items: wire::Array<CountItem>,
    tail: u8,
}

#[derive(WireView, WireBuilder)]
struct PointerBatch {
    marker: u8,
    #[wire(as = i128, le)]
    count: isize,
    #[wire(counted_by = count)]
    items: wire::Array<CountItem>,
    tail: u8,
}

#[derive(WireView, WireBuilder)]
struct NarrowBatch {
    count: i8,
    #[wire(counted_by = count)]
    items: wire::Array<CountItem>,
    tail: u8,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum NarrowEnvelope {
    #[wire(value = 0x6d)]
    Batch(NarrowBatch),
}

fn prove_signed_widths() -> Result<(), Box<dyn std::error::Error>> {
    let mut expected = vec![0, 2, 0x31, 0x42];
    expected.extend_from_slice(&1i64.to_le_bytes());
    expected.push(0x53);
    expected.extend_from_slice(&3i128.to_be_bytes());
    expected.extend_from_slice(&[0x64, 0x75, 0x86, 0x97]);
    let mut output = Vec::new();
    let written = SignedWidths::builder(&mut output)
        .short_items(|items| items.try_extend([0x31, 0x42], |item, value| item.value(value)))?
        .long_items(|items| items.try_extend([0x53], |item, value| item.value(value)))?
        .wide_items(|items| items.try_extend([0x64, 0x75, 0x86], |item, value| item.value(value)))?
        .tail(0x97)?
        .finish()?;
    assert_eq!(written.as_bytes(), expected);
    let view = SignedWidths::view(written.as_bytes())?;
    assert_eq!(view.short_count(), 2i16);
    assert_eq!(view.long_count(), 1i64);
    assert_eq!(view.wide_count(), 3i128);
    assert_eq!(view.tail(), 0x97);
    for (items, expected) in [
        (view.short_items(), &[0x31, 0x42][..]),
        (view.long_items(), &[0x53][..]),
        (view.wide_items(), &[0x64, 0x75, 0x86][..]),
    ] {
        let values = items
            .iter()
            .map(|item| item.map(|item| item.view().value()))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(values, expected);
    }

    let mut expected = vec![0xa8];
    expected.extend_from_slice(&2i128.to_le_bytes());
    expected.extend_from_slice(&[0xb9, 0xca, 0xdb]);
    let mut output = [0; 20];
    let written = PointerBatch::builder(&mut output[..])
        .marker(0xa8)?
        .items(|items| items.try_extend([0xb9, 0xca], |item, value| item.value(value)))?
        .tail(0xdb)?
        .finish()?;
    assert_eq!(written.as_bytes(), expected);
    let view = PointerBatch::view(written.as_bytes())?;
    assert_eq!(view.count(), 2isize);
    assert_eq!(view.items().len(), 2);
    assert_eq!(view.tail(), 0xdb);
    for (count, expected_target) in [(-1i128, "usize"), (i128::MIN, "isize")] {
        let mut input = [0; 17];
        input[0] = 0xa8;
        input[1..].copy_from_slice(&count.to_le_bytes());
        for error in [
            PointerBatch::view(input).err().expect("negative count"),
            PointerBatch::view_unchecked(input)
                .err()
                .expect("unchecked negative count"),
        ] {
            assert!(matches!(
                error,
                PointerBatchViewError::CountValue(wire_repr::ScalarConversionError {
                    offset: 1,
                    from: "i128",
                    to,
                }) if to == expected_target
            ));
        }
    }
    Ok(())
}

fn prove_narrow_detached_count() -> Result<(), Box<dyn std::error::Error>> {
    let mut output = [0x55; 130];
    let written = NarrowEnvelope::builder(&mut output[..])
        .batch(|batch| {
            batch
                .items(|items| items.extend(0..127u8, |item, value| item.value(value)))
                .tail(0xe1)
        })?
        .finish()?;
    let mut expected = vec![0x6d, 0x7f];
    expected.extend(0..127u8);
    expected.push(0xe1);
    assert_eq!(written.as_bytes(), expected);
    let view = NarrowEnvelope::view(written.as_bytes())?;
    let NarrowEnvelopeVariant::Batch(batch) = view.variant();
    assert_eq!(batch.count(), 127i8);
    assert_eq!(batch.tail(), 0xe1);
    let values = batch
        .items()
        .iter()
        .map(|item| item.map(|item| item.view().value()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(values, (0..127u8).collect::<Vec<_>>());

    let mut output = [0x55; 131];
    let error = match NarrowEnvelope::builder(&mut output[..]).batch(|batch| {
        batch
            .items(|items| items.extend(0..128u8, |item, value| item.value(value)))
            .tail(0xe1)
    }) {
        Ok(_) => panic!("i8 count overflow was accepted"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::WriteError::Schema(NarrowEnvelopeBatchWriteError::Body(
            NarrowBatchWriteError::Layout(wire_repr::LayoutError { field: "count" })
        ))
    ));
    let mut expected = vec![0x6d, 0];
    expected.extend(0..128u8);
    expected.push(0x55);
    assert_eq!(output.as_slice(), expected);
    assert!(matches!(
        NarrowEnvelope::view([0x6d, 0xff]),
        Err(NarrowEnvelopeViewError::VariantBatch(
            NarrowBatchViewError::CountValue(wire_repr::ScalarConversionError {
                offset: 1,
                from: "i8",
                to: "usize",
            })
        ))
    ));
    Ok(())
}

#[derive(WireView, WireBuilder)]
struct ParcelData {
    #[wire(uleb128)]
    number: u32,
    #[wire(rest)]
    bytes: wire::Bytes,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum ParcelItem {
    #[wire(value = 0x27)]
    Data(ParcelData),
}

#[derive(WireView, WireBuilder)]
struct ParcelContent {
    #[wire(uleb128)]
    length: u16,
    #[wire(bytes = length)]
    item: ParcelItem,
    tail: u8,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum Parcel {
    #[wire(value = 0x59)]
    Body(ParcelContent),
}

fn prove_uleb_envelope() -> Result<(), Box<dyn std::error::Error>> {
    let payload = [0x6e; 125];
    let mut expected = vec![0x59, 0x80, 0x01, 0x27, 0x80, 0x02];
    expected.extend_from_slice(&payload);
    expected.push(0x96);
    let mut output = [0x55; 132];
    let written = Parcel::builder(&mut output[..])
        .body(|body| {
            body.item(|item| item.data(|data| data.number(256).bytes(payload)))
                .tail(0x96)
        })?
        .finish()?;
    assert_eq!(written.as_bytes(), expected.as_slice());
    let view = Parcel::view(written.as_bytes())?;
    let ParcelVariant::Body(body) = view.variant();
    assert_eq!(body.length(), 128);
    assert_eq!(body.tail(), 0x96);
    let item = body.item();
    let ParcelItemVariant::Data(data) = item.variant();
    assert_eq!(data.number(), 256);
    assert_eq!(data.bytes(), payload);
    Ok(())
}

#[derive(WireView, WireBuilder)]
struct IntegerRecord {
    #[wire(sleb128)]
    delta: i32,
    #[wire(uleb128, zigzag)]
    distance: i16,
    #[wire(vu128)]
    code: u32,
    #[wire(vu128, zigzag)]
    displacement: i16,
    #[wire(vu128)]
    length: u16,
    #[wire(bytes = length)]
    payload: wire::Bytes,
    tail: u8,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum IntegerEnvelope {
    #[wire(value = 0x2e)]
    Record(IntegerRecord),
}

fn prove_integer_formats() -> Result<(), Box<dyn std::error::Error>> {
    let payload = [0xb7; 128];
    let mut expected = vec![
        0x2e, 0x9b, 0xf1, 0x59, 0x81, 0x01, 0xde, 0xe6, 0x55, 0x81, 0x02, 0x80, 0x02,
    ];
    expected.extend_from_slice(&payload);
    expected.push(0x3d);
    let mut output = [0; 142];
    let written = IntegerEnvelope::builder(&mut output[..])
        .record(|record| {
            record
                .delta(-624485)
                .distance(-65)
                .code(0xabcde)
                .displacement(-65)
                .payload(payload)
                .tail(0x3d)
        })?
        .finish()?;
    assert_eq!(written.as_bytes(), expected);
    let view = IntegerEnvelope::view(written.as_bytes())?;
    let IntegerEnvelopeVariant::Record(record) = view.variant();
    assert_eq!(record.delta(), -624485);
    assert_eq!(record.distance(), -65);
    assert_eq!(record.code(), 0xabcde);
    assert_eq!(record.displacement(), -65);
    assert_eq!(record.length(), 128);
    assert_eq!(record.payload(), payload);
    assert_eq!(record.tail(), 0x3d);
    let mut progressive = [0; 141];
    let written = IntegerRecord::builder(&mut progressive[..])
        .delta(-624485)?
        .distance(-65)?
        .code(0xabcde)?
        .displacement(-65)?
        .payload(payload)?
        .tail(0x3d)?
        .finish()?;
    assert_eq!(written.as_bytes(), &expected[1..]);
    Ok(())
}

#[derive(WireView, WireBuilder)]
struct SizedBlock {
    length: u8,
    #[wire(bytes = length)]
    payload: wire::Bytes,
}

#[derive(WireView, WireBuilder)]
struct BlockRecord<T> {
    prefix: u8,
    child: T,
    #[wire(be)]
    suffix: u16,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum BlockEnvelope {
    #[wire(value = 0x78)]
    Record(BlockRecord<SizedBlock>),
}

#[derive(WireView, WireBuilder)]
struct BlockPacket {
    prefix: u8,
    body: BlockEnvelope,
    suffix: u8,
}

fn prove_leading_children() -> Result<(), Box<dyn std::error::Error>> {
    let expected = [0x29, 2, 0x3a, 0x4b, 0x5c, 0x6d];
    let mut output = [0xcc; 6];
    let written = BlockRecord::<SizedBlock>::builder(&mut output[..])
        .prefix(0x29)?
        .child(|child| child.payload(&[0x3a, 0x4b][..]))?
        .suffix(0x5c6d)?
        .finish()?;
    assert_eq!(written.as_bytes(), expected);
    let view = BlockRecord::<SizedBlock>::view(written.as_bytes())?;
    assert_eq!(view.child().payload(), [0x3a, 0x4b]);
    assert_eq!(view.child().as_bytes(), &expected[1..4]);
    assert_eq!(view.suffix(), 0x5c6d);

    let detached = <BlockRecord<SizedBlock> as WireBuilder>::builder()
        .prefix(0x29)
        .child(|child| child.payload(&[0x3a, 0x4b][..]))
        .suffix(0x5c6d);
    let expected = [0x17, 0x78, 0x29, 2, 0x3a, 0x4b, 0x5c, 0x6d, 0x8e];
    let mut output = Vec::new();
    let written = BlockPacket::builder(&mut output)
        .prefix(0x17)?
        .body(|body| body.record(|_| detached))?
        .suffix(0x8e)?
        .finish()?;
    assert_eq!(written.as_bytes(), expected);
    let view = BlockPacket::view(written.as_bytes())?;
    let body = view.body();
    let BlockEnvelopeVariant::Record(record) = body.variant();
    assert_eq!(record.child().payload(), [0x3a, 0x4b]);
    assert_eq!(record.suffix(), 0x5c6d);
    assert_eq!(view.suffix(), 0x8e);

    let input = [0x29, 0, 0x5c, 0x6d, 0x39, 1, 0x4b, 0x6c, 0x7d, 0x8e];
    let (first, mut cursor) = BlockRecord::<SizedBlock>::cursor(&input)?;
    let second = BlockRecord::<SizedBlock>::next(&mut cursor)?;
    assert_eq!(first.child().payload(), []);
    assert_eq!(second.child().payload(), [0x4b]);
    assert_eq!(second.suffix(), 0x6c7d);
    assert_eq!(cursor.remaining(), &[0x8e]);
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let expected = [1, 2, 1, 0xaa, 2, 0xbb, 0xcc, 0x2a];
    let view = Packet::view(expected)?;
    assert_eq!(view.method_count(), 1);
    assert_eq!(view.tail(), 0x2a);
    let mut methods = view.methods().iter();
    let method = methods.next().transpose()?.expect("one method");
    assert_eq!(method.as_bytes(), &expected[1..7]);
    assert!(methods.next().is_none());
    let method = method.view();
    assert_eq!(method.attribute_count(), 2);
    let mut attributes = method.attributes().iter();
    assert_eq!(
        attributes
            .next()
            .transpose()?
            .expect("first attribute")
            .view()
            .data(),
        [0xaa],
    );
    assert_eq!(
        attributes
            .next()
            .transpose()?
            .expect("second attribute")
            .view()
            .data(),
        [0xbb, 0xcc],
    );
    assert!(attributes.next().is_none());

    let mut output = [0u8; 8];
    let written = Packet::builder(&mut output[..])
        .methods(|methods| methods.copy_from(view.methods()))?
        .tail(0x7f)?
        .finish()?;
    assert_eq!(written.as_bytes(), [1, 2, 1, 0xaa, 2, 0xbb, 0xcc, 0x7f]);
    assert_eq!(output, [1, 2, 1, 0xaa, 2, 0xbb, 0xcc, 0x7f]);

    let error = match Packet::view([1, 1, 2, 0xaa]) {
        Ok(_) => panic!("truncated nested item was accepted"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        PacketViewError::Methods(ArrayError::Item {
            index: 0,
            source: MethodViewError::Attributes(ArrayError::Item {
                index: 0,
                source: AttributeViewError::Data(NeedMore {
                    offset: 4,
                    additional_at_least: 1,
                }),
            }),
        })
    ));
    let error = match Packet::view([1, 0, 0x2a, 99]) {
        Ok(_) => panic!("trailing bytes were accepted"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        PacketViewError::Trailing(TrailingBytes {
            offset: 3,
            trailing: 1,
        })
    ));

    let expected = [
        0x31, 0x02, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x11, 0xff, 0xff, 0xff, 0xe9, 0x00,
        0x00, 0x00, 0x29, 0xff, 0xff, 0xff, 0xc5, 0x01,
    ];
    let mut output = Vec::new();
    let written = Envelope::builder(&mut output)
        .batch(|batch| {
            batch
                .entries(|entries| {
                    entries.extend([(17, -23), (41, -59)], |entry, (key, value)| {
                        entry.key(key).value(value)
                    })
                })
                .enabled(true)
        })?
        .finish()?;
    assert_eq!(written.as_bytes(), expected);

    let envelope = Envelope::view(written.as_bytes())?;
    let EnvelopeVariant::Batch(batch) = envelope.variant();
    let count: i32 = batch.count();
    assert_eq!(count, 2);
    assert!(batch.enabled());
    assert_eq!(batch.entries().len(), 2);
    let mut entries = batch.entries().iter();
    let first = entries.next().transpose()?.expect("first entry");
    assert_eq!((first.view().key(), first.view().value()), (17, -23));
    let second = entries.next().transpose()?.expect("second entry");
    assert_eq!((second.view().key(), second.view().value()), (41, -59));
    assert!(entries.next().is_none());

    for count in [-1i32, i32::MIN] {
        let mut input = [0u8; 6];
        input[..2].copy_from_slice(&[0x31, 0x02]);
        input[2..].copy_from_slice(&count.to_be_bytes());
        assert!(matches!(
            Envelope::view(input),
            Err(EnvelopeViewError::VariantBatch(BatchViewError::CountValue(
                wire_repr::ScalarConversionError {
                    offset: 2,
                    from: "i32",
                    to: "usize",
                }
            )))
        ));
    }
    prove_uleb_envelope()?;
    prove_integer_formats()?;
    prove_signed_widths()?;
    prove_narrow_detached_count()?;
    prove_leading_children()?;
    Ok(())
}
