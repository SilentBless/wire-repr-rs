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
    count: u32,
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
    assert_eq!(batch.count(), 2);
    assert!(batch.enabled());
    assert_eq!(batch.entries().len(), 2);
    let mut entries = batch.entries().iter();
    let first = entries.next().transpose()?.expect("first entry");
    assert_eq!((first.view().key(), first.view().value()), (17, -23));
    let second = entries.next().transpose()?.expect("second entry");
    assert_eq!((second.view().key(), second.view().value()), (41, -59));
    assert!(entries.next().is_none());
    prove_uleb_envelope()?;
    prove_integer_formats()?;
    Ok(())
}
