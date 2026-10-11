#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView, select};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(WireView, WireBuilder)]
struct Bar {
    #[wire(be)]
    value: u16,
}

#[derive(WireView, WireBuilder)]
struct ConstantBar {
    #[wire(constant = 1)]
    tag: u8,
    value: u8,
}

#[derive(WireView, WireBuilder)]
struct Foo<T> {
    count: u8,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<T>,
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
    entries: wire_repr::wire::Array<Entry>,
    #[wire(as = u8)]
    enabled: bool,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u16, be)]
pub enum Envelope {
    #[wire(value = 0x3102)]
    Batch(Batch),
}

const BATCH_BYTES: [u8; 23] = [
    0x31, 0x02, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x11, 0xff, 0xff, 0xff, 0xe9, 0x00, 0x00,
    0x00, 0x29, 0xff, 0xff, 0xff, 0xc5, 0x01,
];

#[test]
fn enum_body_streams_detached_entries_into_fixed_and_growing_outputs() -> TestResult {
    let mut fixed = [0u8; 23];
    let written = Envelope::builder(&mut fixed[..])
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
    assert_eq!(written.as_bytes(), BATCH_BYTES);
    assert_eq!(fixed, BATCH_BYTES);

    let view = Envelope::view(fixed)?;
    let EnvelopeVariant::Batch(batch) = view.variant();
    assert_eq!(batch.count(), 2);
    assert!(batch.enabled());
    assert_eq!(batch.entries().len(), 2);
    let mut entries = batch.entries().iter();
    let first = entries.next().transpose()?.expect("first entry");
    assert_eq!((first.view().key(), first.view().value()), (17, -23));
    let second = entries.next().transpose()?.expect("second entry");
    assert_eq!((second.view().key(), second.view().value()), (41, -59));
    assert!(entries.next().is_none());

    let mut growing = Vec::new();
    let written = Envelope::builder(&mut growing)
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
    assert_eq!(written.as_bytes(), BATCH_BYTES);
    Ok(())
}

#[test]
fn empty_detached_array_patches_zero_count_and_preserves_the_enum_body_tail() -> TestResult {
    let mut output = [0xff; 7];
    Envelope::builder(&mut output[..])
        .batch(|batch| {
            batch
                .entries(|entries| {
                    entries.extend(core::iter::empty::<(i32, i32)>(), |entry, (key, value)| {
                        entry.key(key).value(value)
                    })
                })
                .enabled(false)
        })?
        .finish()?;
    assert_eq!(output, [0x31, 0x02, 0, 0, 0, 0, 0]);
    let view = Envelope::view(output)?;
    let EnvelopeVariant::Batch(batch) = view.variant();
    assert_eq!(batch.count(), 0);
    assert!(!batch.enabled());
    assert!(batch.entries().is_empty());
    assert!(batch.entries().iter().next().is_none());
    Ok(())
}

#[test]
fn detached_array_defers_into_iterator_and_mutable_item_callbacks_until_writing() -> TestResult {
    use core::cell::Cell;

    struct Source<'a> {
        started: &'a Cell<usize>,
    }

    impl IntoIterator for Source<'_> {
        type Item = (i32, i32);
        type IntoIter = std::array::IntoIter<Self::Item, 2>;

        fn into_iter(self) -> Self::IntoIter {
            self.started.set(self.started.get() + 1);
            [(17, -23), (41, -59)].into_iter()
        }
    }

    let started = Cell::new(0);
    let mut built = 0;
    let batch = <Batch as WireBuilder>::builder()
        .entries(|entries| {
            entries.extend(Source { started: &started }, |entry, (key, value)| {
                built += 1;
                entry.key(key).value(value)
            })
        })
        .enabled(true);
    assert_eq!(started.get(), 0);

    let mut output = [0u8; 23];
    Envelope::builder(&mut output[..])
        .batch(|_| batch)?
        .finish()?;
    assert_eq!(started.get(), 1);
    assert_eq!(built, 2);
    assert_eq!(output, BATCH_BYTES);
    Ok(())
}

#[test]
fn detached_enum_array_reports_fixed_output_shortages_at_every_boundary() {
    for available in 0..BATCH_BYTES.len() {
        let mut output = [0x55; 23];
        let error = match Envelope::builder(&mut output[..available]).batch(|batch| {
            batch
                .entries(|entries| {
                    entries.extend([(17, -23), (41, -59)], |entry, (key, value)| {
                        entry.key(key).value(value)
                    })
                })
                .enabled(true)
        }) {
            Ok(_) => panic!("short output with {available} bytes unexpectedly accepted"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            wire_repr::WriteError::Output(wire_repr::OutputError::NeedMore {
                request,
                available: reported,
            }) if reported == available && request.minimum_len > available
        ));
        if available == 22 {
            assert_eq!(&output[..22], &BATCH_BYTES[..22]);
            assert_eq!(output[22], 0x55);
        }
    }
}

#[test]
fn nested_detached_array_conversion_failure_keeps_field_sources_and_stops_streaming() {
    #[derive(WireView, WireBuilder)]
    struct CheckedItem {
        #[wire(as = u8)]
        value: usize,
    }

    let consumed = core::cell::Cell::new(0);
    let source = [1usize, 256, 2].into_iter().inspect(|_| {
        consumed.set(consumed.get() + 1);
    });
    let mut output = [0x55; 6];
    let error = match Foo::<Foo<CheckedItem>>::builder(&mut output[..]).items(|items| {
        items.item(|group| {
            group
                .items(|items| items.extend(source, |item, value| item.value(value)))
                .tail(7)
        })
    }) {
        Ok(_) => panic!("out-of-range detached array item unexpectedly wrote"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::WriteError::Schema(FooWriteError::Items(FooWriteError::Items(
            CheckedItemWriteError::ValueValue(_)
        )))
    ));
    assert_eq!(consumed.get(), 2);
    assert_eq!(output, [0, 0, 1, 0x55, 0x55, 0x55]);
}

#[test]
fn detached_enum_array_count_overflow_keeps_the_variant_and_controller_field_source() {
    let mut output = [0x55; 258];
    let error = match MethodChoice::builder(&mut output[..]).present(|method| {
        method.attributes(|attributes| attributes.extend(0..256, |attribute, _| attribute.data([])))
    }) {
        Ok(_) => panic!("unrepresentable detached array count unexpectedly wrote"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::WriteError::Schema(MethodChoicePresentWriteError::Body(
            MethodWriteError::Layout(wire_repr::LayoutError {
                field: "attributes_count"
            })
        ))
    ));
    let mut expected = [0u8; 258];
    expected[0] = 1;
    assert_eq!(output, expected);
}

#[test]
fn nested_detached_array_rejects_a_zero_width_item_without_wrapping_the_output_error() {
    #[derive(WireView, WireBuilder)]
    struct EmptyItem {
        bytes: [u8; 0],
    }

    let mut output = [0x55; 4];
    let error = match Foo::<Foo<EmptyItem>>::builder(&mut output[..]).items(|items| {
        items.item(|group| {
            group
                .items(|items| items.extend([[]], |item, bytes| item.bytes(bytes)))
                .tail(7)
        })
    }) {
        Ok(_) => panic!("zero-width detached array item unexpectedly wrote"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::WriteError::Output(wire_repr::OutputError::NonProgressItem { index: 0 })
    ));
    assert_eq!(output, [0, 0, 0x55, 0x55]);
}

#[test]
fn detached_array_manual_items_receive_isolated_byte_spans() -> TestResult {
    struct ChecksummedByte;

    impl WireBuilder for ChecksummedByte {
        type Builder = ();

        fn builder() -> Self::Builder {}
    }

    impl wire_repr::WireWrite<u8> for ChecksummedByte {
        type Error = core::convert::Infallible;

        fn write<O: wire_repr::Output>(
            value: u8,
            writer: &mut wire_repr::ChildWriter<'_, O>,
        ) -> Result<(), wire_repr::WriteError<Self::Error, O::GrowError>> {
            writer.write(&[value])?;
            let checksum = writer.as_bytes().iter().copied().fold(0, u8::wrapping_add);
            writer.write(&[checksum])?;
            Ok(())
        }
    }

    let mut output = [0u8; 8];
    Foo::<Foo<ChecksummedByte>>::builder(&mut output[..])
        .items(|groups| {
            groups.item(|group| {
                group
                    .items(|items| items.extend([3, 7], |(), value| value))
                    .tail(9)
            })
        })?
        .tail(10)?
        .finish()?;
    assert_eq!(output, [1, 2, 3, 3, 7, 7, 9, 10]);
    Ok(())
}

#[derive(WireView, WireBuilder)]
struct ExceptionHandler {
    #[wire(be)]
    start_pc: u16,
    #[wire(be)]
    end_pc: u16,
}

#[derive(WireView, WireBuilder)]
struct Code {
    #[wire(be)]
    code_length: u32,
    #[wire(bytes = code_length)]
    instructions: wire_repr::wire::Bytes,
    #[wire(be)]
    exception_count: u16,
    #[wire(counted_by = exception_count)]
    exceptions: wire_repr::wire::Array<ExceptionHandler>,
}

#[derive(WireView, WireBuilder)]
struct CodeWithTail {
    #[wire(be)]
    code_length: u32,
    #[wire(bytes = code_length)]
    instructions: wire_repr::wire::Bytes,
    #[wire(be)]
    exception_count: u16,
    marker: u8,
    #[wire(counted_by = exception_count)]
    exceptions: wire_repr::wire::Array<ExceptionHandler>,
    tail: u8,
}

#[test]
fn counted_array_after_dynamic_bytes_round_trips() -> TestResult {
    let input = [0, 0, 0, 2, 0xaa, 0xbb, 0, 1, 0, 3, 0, 5];
    let view = Code::view(input)?;
    assert_eq!(view.code_length(), 2);
    assert_eq!(view.instructions(), [0xaa, 0xbb]);
    assert_eq!(view.exception_count(), 1);
    let handler = view
        .exceptions()
        .iter()
        .next()
        .transpose()?
        .expect("handler");
    assert_eq!(handler.view().start_pc(), 3);
    assert_eq!(handler.view().end_pc(), 5);

    let selected = select(&view).include(|fields| fields.exception_count | fields.exceptions);
    assert_eq!(selected.bytes().collect::<Vec<_>>(), input[6..]);

    let mut output = [0u8; 12];
    Code::builder(&mut output[..])
        .instructions([0xaa, 0xbb])?
        .exceptions(|items| items.item(|item| item.start_pc(3).end_pc(5)))?
        .finish()?;
    assert_eq!(output, input);
    Ok(())
}

#[test]
fn dynamic_count_controller_before_intervening_fields_and_tail() -> TestResult {
    let input = [0, 0, 0, 1, 0xaa, 0, 1, 9, 0, 3, 0, 5, 7];
    let view = CodeWithTail::view(input)?;
    assert_eq!(view.exception_count(), 1);
    assert_eq!(view.marker(), 9);
    assert_eq!(view.tail(), 7);
    assert_eq!(
        view.exceptions()
            .iter()
            .next()
            .transpose()?
            .expect("handler")
            .view()
            .end_pc(),
        5
    );

    let mut output = [0u8; 13];
    CodeWithTail::builder(&mut output[..])
        .instructions([0xaa])?
        .marker(9)?
        .exceptions(|items| items.copy_from(view.exceptions()))?
        .tail(7)?
        .finish()?;
    assert_eq!(output, input);

    let error = match CodeWithTail::view(&input[..6]) {
        Ok(_) => panic!("truncated dynamic count unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        CodeWithTailViewError::ExceptionCount(wire_repr::NeedMore {
            offset: 6,
            additional_at_least: 1,
        })
    ));

    let error = match CodeWithTail::view(&input[..11]) {
        Ok(_) => panic!("truncated array unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        CodeWithTailViewError::Exceptions(wire_repr::ArrayError::NeedMore(wire_repr::NeedMore {
            offset: 11,
            additional_at_least: 1,
        }))
    ));
    Ok(())
}

#[test]
fn fixed_items_replay_exact_views_without_an_index() -> TestResult {
    let input = [2, 0x11, 0x22, 0x33, 0x44, 9];
    let view = Foo::<Bar>::view(input)?;
    let items = view.items();
    assert_eq!(items.len(), 2);
    assert!(!items.is_empty());

    let mut iter = items.iter();
    let first = iter.next().transpose()?.expect("first item");
    let second = iter.next().transpose()?.expect("second item");
    assert_eq!(first.view().value(), 0x1122);
    assert_eq!(second.view().value(), 0x3344);
    assert!(iter.next().is_none());
    assert_eq!(view.tail(), 9);

    let replayed = items.iter().next().transpose()?.expect("replayed item");
    assert_eq!(replayed.view().value(), 0x1122);
    Ok(())
}

#[test]
fn array_views_follow_into_iterator_and_fused_iterator_conventions() -> TestResult {
    let view = Foo::<Bar>::view([2, 0x11, 0x22, 0x33, 0x44, 9])?;
    let items = view.items();
    let mut borrowed = (&items).into_iter();
    fn assert_fused<I: core::iter::FusedIterator>(_iter: &I) {}
    assert_fused(&borrowed);
    assert_eq!(
        borrowed.next().transpose()?.expect("first").view().value(),
        0x1122
    );

    let values = view
        .items()
        .into_iter()
        .map(|item| item.map(|item| item.view().value()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(values, [0x1122, 0x3344]);
    Ok(())
}

#[test]
fn streaming_writer_patches_count_and_writes_each_item_progressively() -> TestResult {
    let mut output = [0u8; 6];
    let source = vec![0x1122, 0x3344];
    Foo::<Bar>::builder(&mut output[..])
        .items(|items| items.try_extend(source, |bar, value| bar.value(value)))?
        .tail(9)?
        .finish()?;
    assert_eq!(output, [2, 0x11, 0x22, 0x33, 0x44, 9]);
    Ok(())
}

#[test]
fn exact_array_forwarding_copies_one_validated_range_and_patches_count() -> TestResult {
    let source = Foo::<Bar>::view([2, 0x11, 0x22, 0x33, 0x44, 9])?;
    let mut output = [0u8; 6];
    Foo::<Bar>::builder(&mut output[..])
        .items(|items| items.copy_from(source.items()))?
        .tail(7)?
        .finish()?;
    assert_eq!(output, [2, 0x11, 0x22, 0x33, 0x44, 7]);
    Ok(())
}

#[test]
fn unrepresentable_streamed_count_returns_a_typed_controller_error() {
    let mut output = [0u8; 514];
    let writer = Foo::<Bar>::builder(&mut output[..]);
    let error = match writer.items(|mut items| {
        for index in 0..256u16 {
            items = items.item(|bar| bar.value(index))?;
        }
        Ok(items)
    }) {
        Ok(_) => panic!("unrepresentable item count unexpectedly wrote"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::WriteError::Schema(FooWriteError::Layout(wire_repr::LayoutError {
            field: "count"
        }))
    ));
}
#[derive(WireView, WireBuilder)]
struct Variable {
    length: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
}

#[derive(WireView)]
struct Terminal<T> {
    count: u8,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<T>,
}

#[derive(WireView, WireBuilder)]
struct PlacedTerminal<T> {
    count: u8,
    #[wire(counted_by = count, at = 10)]
    items: wire_repr::wire::Array<T>,
}

#[derive(WireView, WireBuilder)]
struct Attribute {
    length: u8,
    #[wire(bytes = length)]
    data: wire_repr::wire::Bytes,
}

#[derive(WireView, WireBuilder)]
struct Method {
    attributes_count: u8,
    #[wire(counted_by = attributes_count)]
    attributes: wire_repr::wire::Array<Attribute>,
}

#[derive(WireView)]
struct Class {
    methods_count: u8,
    #[wire(counted_by = methods_count)]
    methods: wire_repr::wire::Array<Method>,
    tail: u8,
}

#[derive(WireView, WireBuilder)]
struct MethodWithCode {
    code_length: u8,
    #[wire(bytes = code_length)]
    code: wire_repr::wire::Bytes,
    attributes_count: u8,
    #[wire(counted_by = attributes_count)]
    attributes: wire_repr::wire::Array<Attribute>,
}

#[derive(WireView, WireBuilder)]
struct ClassWithCode {
    methods_count: u8,
    #[wire(counted_by = methods_count)]
    methods: wire_repr::wire::Array<MethodWithCode>,
    tail: u8,
}

#[derive(WireView)]
struct MethodWrapper {
    method: Method,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum MethodChoice {
    #[wire(value = 1)]
    Present(Method),
    #[wire(value = 2)]
    Empty,
}

#[derive(WireView)]
#[wire(selector = u8)]
enum OpenMethodChoice {
    #[wire(value = 1)]
    Present(Method),
    #[wire(unknown)]
    Unknown(wire_repr::wire::Bytes),
}

#[derive(WireView)]
struct UnboundedAttribute {
    #[wire(rest)]
    data: wire_repr::wire::Bytes,
}

#[derive(WireView)]
struct UnboundedMethod {
    attributes_count: u8,
    #[wire(counted_by = attributes_count)]
    attributes: wire_repr::wire::Array<UnboundedAttribute>,
}

#[derive(WireView)]
struct UnboundedClass {
    methods_count: u8,
    #[wire(counted_by = methods_count)]
    methods: wire_repr::wire::Array<UnboundedMethod>,
    tail: u8,
}

#[test]
fn nested_counted_arrays_stop_before_following_fields() -> TestResult {
    assert_eq!(
        <Method as wire_repr::WireView>::frame(&[0, 0x2a], 0)?
            .into_parts()
            .1,
        1,
    );
    let view = Class::view([1, 0, 0x2a])?;
    assert_eq!(view.tail(), 42);
    let mut methods = view.methods().iter();
    let method = methods.next().transpose()?.expect("one method");
    assert_eq!(method.as_bytes(), &[0]);
    assert_eq!(method.view().attributes_count(), 0);
    assert!(method.view().attributes().iter().next().is_none());
    assert!(methods.next().is_none());

    let view = Class::view([2, 2, 1, 0xaa, 2, 0xbb, 0xcc, 1, 0, 0x2a])?;
    assert_eq!(view.tail(), 42);
    let mut methods = view.methods().iter();
    let first = methods.next().transpose()?.expect("first method");
    assert_eq!(first.as_bytes(), &[2, 1, 0xaa, 2, 0xbb, 0xcc]);
    let first_view = first.view();
    let mut attributes = first_view.attributes().iter();
    assert_eq!(
        attributes
            .next()
            .transpose()?
            .expect("first attribute")
            .view()
            .data(),
        [0xaa]
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
    let second = methods.next().transpose()?.expect("second method");
    assert_eq!(second.as_bytes(), &[1, 0]);
    assert_eq!(
        second
            .view()
            .attributes()
            .iter()
            .next()
            .transpose()?
            .expect("empty attribute")
            .view()
            .data(),
        [],
    );
    assert!(methods.next().is_none());
    Ok(())
}

#[test]
fn counted_prefix_after_dynamic_bytes_preserves_outer_tail() -> TestResult {
    let view = ClassWithCode::view([1, 2, 0xa1, 0xa2, 0, 42])?;
    assert_eq!(view.tail(), 42);
    let method = view.methods().iter().next().transpose()?.expect("method");
    assert_eq!(method.as_bytes(), [2, 0xa1, 0xa2, 0]);
    assert_eq!(method.view().code(), [0xa1, 0xa2]);

    let view = ClassWithCode::view([1, 2, 0xa1, 0xa2, 1, 0, 42])?;
    assert_eq!(view.tail(), 42);
    assert_eq!(
        view.methods()
            .iter()
            .next()
            .transpose()?
            .expect("method")
            .as_bytes(),
        [2, 0xa1, 0xa2, 1, 0],
    );
    Ok(())
}

#[test]
fn multiple_detached_array_levels_patch_counts_after_dynamic_data() -> TestResult {
    let expected = [1, 2, 2, 0xa1, 0xa2, 2, 1, 0xaa, 2, 0xbb, 0xcc, 0, 0, 42, 99];
    let mut output = [0u8; 15];
    let attributes = [&[0xaa][..], &[0xbb, 0xcc][..]];
    Foo::<ClassWithCode>::builder(&mut output[..])
        .items(|classes| {
            classes.item(|class| {
                class
                    .methods(|methods| {
                        methods.extend(
                            [(&[0xa1, 0xa2][..], &attributes[..]), (&[][..], &[][..])],
                            |method, (code, data)| {
                                method.code(code).attributes(|attributes| {
                                    attributes.extend(data, |attribute, data| attribute.data(data))
                                })
                            },
                        )
                    })
                    .tail(42)
            })
        })?
        .tail(99)?
        .finish()?;
    assert_eq!(output, expected);

    let outer = Foo::<ClassWithCode>::view(output)?;
    assert_eq!(outer.count(), 1);
    assert_eq!(outer.tail(), 99);
    let class = outer.items().iter().next().transpose()?.expect("class");
    let class = class.view();
    assert_eq!(class.methods_count(), 2);
    assert_eq!(class.tail(), 42);
    let mut methods = class.methods().iter();
    let first = methods.next().transpose()?.expect("first method");
    let first = first.view();
    assert_eq!(first.code(), [0xa1, 0xa2]);
    assert_eq!(first.attributes_count(), 2);
    let mut attributes = first.attributes().iter();
    assert_eq!(
        attributes
            .next()
            .transpose()?
            .expect("first attribute")
            .view()
            .data(),
        [0xaa]
    );
    assert_eq!(
        attributes
            .next()
            .transpose()?
            .expect("second attribute")
            .view()
            .data(),
        [0xbb, 0xcc]
    );
    assert!(attributes.next().is_none());
    let second = methods.next().transpose()?.expect("second method");
    assert_eq!(second.as_bytes(), [0, 0]);
    assert_eq!(second.view().attributes_count(), 0);
    assert!(second.view().attributes().is_empty());
    assert!(methods.next().is_none());
    Ok(())
}

#[test]
fn terminal_array_prefix_capability_propagates_through_structs_and_enums() -> TestResult {
    let mut methods = Method::views(&[0, 1, 0])?;
    assert_eq!(methods.next()?.expect("first method").as_bytes(), [0]);
    assert_eq!(methods.next()?.expect("second method").as_bytes(), [1, 0]);
    assert!(methods.next()?.is_none());

    let wrapper = MethodWrapper::view([0, 0x2a])?;
    assert_eq!(wrapper.method().attributes_count(), 0);
    let choice = MethodChoice::view([1, 0, 0x2a])?;
    assert!(matches!(
        choice.variant(),
        MethodChoiceVariant::Present(method) if method.attributes_count() == 0
    ));
    let open = OpenMethodChoice::view([1, 0, 0x2a])?;
    assert!(matches!(
        open.variant(),
        OpenMethodChoiceVariant::Present(method) if method.attributes_count() == 0
    ));

    let mut wrappers = MethodWrapper::views(&[0, 0])?;
    assert_eq!(
        wrappers.next()?.expect("first").method().attributes_count(),
        0
    );
    assert_eq!(
        wrappers
            .next()?
            .expect("second")
            .method()
            .attributes_count(),
        0
    );
    assert!(wrappers.next()?.is_none());

    let mut choices = MethodChoice::views(&[1, 0, 2])?;
    let first = choices.next()?.expect("first choice");
    match first.variant() {
        MethodChoiceVariant::Present(method) => assert_eq!(method.attributes_count(), 0),
        MethodChoiceVariant::Empty => panic!("expected method"),
    }
    assert!(matches!(
        choices.next()?.expect("second choice").variant(),
        MethodChoiceVariant::Empty
    ));
    assert!(choices.next()?.is_none());
    Ok(())
}

#[test]
fn nested_array_shortages_keep_absolute_offsets_without_eager_root_validation() -> TestResult {
    let error = match Class::view([1, 1, 2, 0xaa]) {
        Ok(_) => panic!("truncated nested attribute unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        ClassViewError::Methods(wire_repr::ArrayError::Item {
            index: 0,
            source: MethodViewError::Attributes(wire_repr::ArrayError::Item {
                index: 0,
                source: AttributeViewError::Data(wire_repr::NeedMore {
                    offset: 4,
                    additional_at_least: 1,
                }),
            }),
        })
    ));
    let error = match Class::view([1, 1, 0]) {
        Ok(_) => panic!("missing tail unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        ClassViewError::Tail(wire_repr::NeedMore {
            offset: 3,
            additional_at_least: 1,
        })
    ));
    let error = match Class::view([1, 0, 0x2a, 99]) {
        Ok(_) => panic!("trailing bytes unexpectedly accepted"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        ClassViewError::Trailing(wire_repr::TrailingBytes {
            offset: 3,
            trailing: 1,
        })
    ));

    let method = Method::view([1, 2, 0xaa])?;
    assert!(matches!(
        method.attributes().iter().next(),
        Some(Err(wire_repr::ArrayError::Item { index: 0, .. }))
    ));
    let method = Method::view([0, 0x2a])?;
    assert!(matches!(
        method.attributes().iter().next(),
        Some(Err(wire_repr::ArrayError::Trailing {
            offset: 1,
            trailing: 1
        }))
    ));
    Ok(())
}

#[test]
fn nonleading_items_never_claim_a_following_field() -> TestResult {
    assert_eq!(UnboundedClass::view([0, 0x2a])?.tail(), 42);
    let error = match UnboundedClass::view([1, 1, 7, 0x2a]) {
        Ok(_) => panic!("nonleading item unexpectedly framed before tail"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        UnboundedClassViewError::LayoutUnavailable { field: "methods" }
    ));
    Ok(())
}

#[test]
fn ordinary_generic_array_writer_keeps_placement_geometry() -> TestResult {
    let mut output = [0xff; 12];
    let written = PlacedTerminal::<Bar>::builder(&mut output[..])
        .items(|items| items.item(|bar| bar.value(0x1122)))?
        .finish()?;
    assert_eq!(
        written.as_bytes(),
        [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x11, 0x22]
    );
    Ok(())
}

#[test]
fn terminal_array_iterator_is_fused_after_item_failure() -> TestResult {
    let view = Terminal::<Variable>::view([2, 1, 7, 1])?;
    let mut iter = view.items().iter();
    assert!(iter.next().transpose()?.is_some());
    assert!(matches!(
        iter.next(),
        Some(Err(wire_repr::ArrayError::Item { index: 1, .. }))
    ));
    assert!(iter.next().is_none());
    Ok(())
}

#[test]
fn terminal_array_forwarding_validates_deferred_item_geometry_before_copying() -> TestResult {
    let source = Terminal::<Variable>::view([2, 1, 7, 1])?;
    let mut output = [0u8; 8];
    let error = match Foo::<Variable>::builder(&mut output[..])
        .items(|items| items.copy_from(source.items()))
    {
        Ok(_) => panic!("malformed terminal array unexpectedly copied"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::WriteError::Schema(FooWriteError::Items(wire_repr::ArrayError::Item {
            index: 1,
            ..
        }))
    ));
    Ok(())
}

#[test]
fn valid_terminal_array_forwards_exact_items_count_and_new_tail() -> TestResult {
    let source = Terminal::<Variable>::view([2, 1, 7, 2, 8, 9])?;
    let mut output = [0u8; 7];
    Foo::<Variable>::builder(&mut output[..])
        .items(|items| items.copy_from(source.items()))?
        .tail(5)?
        .finish()?;
    assert_eq!(output, [2, 1, 7, 2, 8, 9, 5]);
    Ok(())
}

#[test]
fn fixed_array_forwarding_revalidates_item_constants_before_copying() -> TestResult {
    let source = Terminal::<ConstantBar>::view([1, 2, 0])?;
    let mut output = [0u8; 4];
    let error = match Foo::<ConstantBar>::builder(&mut output[..])
        .items(|items| items.copy_from(source.items()))
    {
        Ok(_) => panic!("invalid fixed item unexpectedly copied"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::WriteError::Schema(FooWriteError::Items(wire_repr::ArrayError::Item {
            index: 0,
            ..
        }))
    ));
    Ok(())
}

#[test]
fn terminal_fixed_array_forwarding_preserves_exact_shortage_precedence() -> TestResult {
    let source = Terminal::<Bar>::view([2, 0x11, 0x22])?;
    let mut output = [0u8; 6];
    let error =
        match Foo::<Bar>::builder(&mut output[..]).items(|items| items.copy_from(source.items())) {
            Ok(_) => panic!("truncated fixed array unexpectedly copied"),
            Err(error) => error,
        };
    assert!(matches!(
        error,
        wire_repr::WriteError::Schema(FooWriteError::Items(wire_repr::ArrayError::NeedMore(
            wire_repr::NeedMore {
                additional_at_least: 2,
                ..
            }
        )))
    ));
    Ok(())
}

#[test]
fn terminal_array_placement_beyond_input_returns_shortage_before_getter() {
    let error = match PlacedTerminal::<Bar>::view([0]) {
        Ok(_) => panic!("out-of-input terminal array unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        PlacedTerminalViewError::Items(wire_repr::ArrayError::NeedMore(wire_repr::NeedMore {
            offset: 1,
            additional_at_least: 9,
        }))
    ));
}

#[test]
fn variable_items_frame_one_exact_item_at_a_time() -> TestResult {
    let input = [2, 2, 1, 2, 1, 3, 9];
    let view = Foo::<Variable>::view(input)?;
    let mut iter = view.items().iter();
    let first = iter.next().transpose()?.expect("first item");
    let second = iter.next().transpose()?.expect("second item");
    assert_eq!(first.view().body(), &[1, 2]);
    assert_eq!(second.view().body(), &[3]);
    assert_eq!(view.tail(), 9);
    Ok(())
}

#[test]
fn variable_item_failure_keeps_array_index_and_parent_field_site() {
    let error = match Foo::<Variable>::view([2, 2, 1, 2, 1]) {
        Ok(_) => panic!("truncated second item unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        FooViewError::Items(wire_repr::ArrayError::Item { index: 1, .. })
    ));
}

#[test]
fn fixed_item_count_shortage_is_exact() {
    let error = match Foo::<Bar>::view([3, 0x11, 0x22, 0x33, 0x44]) {
        Ok(_) => panic!("counted fixed array shortage unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        FooViewError::Items(wire_repr::ArrayError::NeedMore(wire_repr::NeedMore {
            additional_at_least: 2,
            ..
        }))
    ));
}

#[test]
fn exact_item_views_stream_without_semantic_reconstruction() -> TestResult {
    let input = [2, 0x11, 0x22, 0x33, 0x44, 9];
    let source = Foo::<Bar>::view(input)?;
    let mut output = [0u8; 6];
    Foo::<Bar>::builder(&mut output[..])
        .items(|mut output_items| {
            for item in source.items().iter() {
                output_items = output_items.item_result(item)?;
            }
            Ok(output_items)
        })?
        .tail(9)?
        .finish()?;
    assert_eq!(output, input);
    Ok(())
}

#[test]
fn ordinary_generated_views_share_the_exact_item_write_capability() -> TestResult {
    let first = Bar::view([0x11, 0x22])?;
    let second = Bar::view([0x33, 0x44])?;
    let mut output = [0u8; 6];
    Foo::<Bar>::builder(&mut output[..])
        .items(|items| {
            let items = items.item_view(first)?;
            let items = items.item_view(second)?;
            Ok(items)
        })?
        .tail(9)?
        .finish()?;
    assert_eq!(output, [2, 0x11, 0x22, 0x33, 0x44, 9]);
    Ok(())
}
