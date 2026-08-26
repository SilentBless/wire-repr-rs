#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(WireView, WireBuilder)]
struct Bar {
    #[wire(be)]
    value: u16,
}

#[derive(WireView, WireBuilder)]
struct Foo<T> {
    count: u8,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<T>,
    tail: u8,
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
fn streaming_writer_patches_count_and_writes_each_item_progressively() -> TestResult {
    let mut output = [0u8; 6];
    Foo::<Bar>::builder(&mut output[..])
        .items(|items| {
            let items = items.item(|bar| bar.value(0x1122))?;
            let items = items.item(|bar| bar.value(0x3344))?;
            Ok(items)
        })?
        .tail(9)?
        .finish()?;
    assert_eq!(output, [2, 0x11, 0x22, 0x33, 0x44, 9]);
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
