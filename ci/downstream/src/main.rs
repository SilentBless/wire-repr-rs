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
    Ok(())
}
