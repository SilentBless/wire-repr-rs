#![allow(dead_code)]

use wire_repr::{WireView, select};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(WireView)]
struct Foo {
    first: u8,
    length: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
    tail: u8,
}

#[derive(WireView)]
struct NamedBytes {
    bytes: [u8; 2],
    tail: u8,
}

#[test]
fn include_preserves_physical_order_independent_of_expression_order() -> TestResult {
    let view = Foo::view([1, 2, 10, 11, 9])?;
    let selected = select(&view).include(|fields| fields.tail | fields.first);
    assert_eq!(selected.bytes().collect::<Vec<_>>(), vec![1, 9]);
    assert_eq!(selected.len(), 2);
    Ok(())
}

#[test]
fn chunks_merge_adjacent_ranges_without_materializing_bytes() -> TestResult {
    let view = Foo::view([1, 2, 10, 11, 9])?;
    let selected = select(&view).include(|fields| fields.first | fields.length | fields.body);
    assert_eq!(
        selected.chunks().collect::<Vec<_>>(),
        vec![&[1, 2, 10, 11][..]]
    );
    Ok(())
}

#[test]
fn exclude_keeps_every_unselected_physical_span() -> TestResult {
    let view = Foo::view([1, 2, 10, 11, 9])?;
    let selected = select(&view).exclude(|fields| fields.body);
    assert_eq!(selected.bytes().collect::<Vec<_>>(), vec![1, 2, 9]);
    assert_eq!(
        selected.chunks().collect::<Vec<_>>(),
        vec![&[1, 2][..], &[9][..]],
    );
    Ok(())
}

#[test]
fn free_selection_entrypoint_never_reserves_a_field_method_name() -> TestResult {
    let view = NamedBytes::view([1, 2, 3])?;
    assert_eq!(view.bytes(), [1, 2]);
    let selected = select(&view).include(|fields| fields.bytes);
    assert_eq!(selected.bytes().collect::<Vec<_>>(), vec![1, 2]);
    Ok(())
}
