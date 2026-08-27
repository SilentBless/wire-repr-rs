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

#[derive(WireView)]
struct Leaf {
    head: u8,
    length: u8,
    #[wire(bytes = length)]
    body: wire_repr::wire::Bytes,
    tail: u8,
}

#[derive(WireView)]
struct Middle {
    leaf_length: u8,
    #[wire(bytes = leaf_length)]
    leaf: Leaf,
    end: u8,
}

#[derive(WireView)]
struct Root {
    middle_length: u8,
    #[wire(bytes = middle_length)]
    middle: Middle,
    suffix: u8,
}

#[derive(WireView)]
struct GenericRoot<T> {
    marker: u8,
    child: T,
}

fn nested_view() -> Result<impl RootView, Box<dyn std::error::Error>> {
    Ok(Root::view([7, 5, 10, 2, 20, 21, 11, 12, 99])?)
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

#[test]
fn nested_paths_resolve_to_root_relative_dynamic_ranges() -> TestResult {
    let view = nested_view()?;
    let selected = select(&view).include(|fields| {
        fields
            .middle
            .fields(|middle| middle.leaf.fields(|leaf| leaf.body) | middle.end)
    });
    assert_eq!(
        selected.chunks().collect::<Vec<_>>(),
        vec![&[20, 21][..], &[12][..]],
    );
    Ok(())
}

#[test]
fn nested_paths_preserve_root_physical_order_and_exclusion() -> TestResult {
    let view = nested_view()?;
    let included = select(&view).include(|fields| {
        fields.suffix
            | fields
                .middle
                .fields(|middle| middle.leaf.fields(|leaf| leaf.body))
            | fields.middle_length
    });
    assert_eq!(included.bytes().collect::<Vec<_>>(), vec![7, 20, 21, 99]);

    let excluded = select(&view).exclude(|fields| {
        fields
            .middle
            .fields(|middle| middle.leaf.fields(|leaf| leaf.body))
    });
    assert_eq!(
        excluded.bytes().collect::<Vec<_>>(),
        vec![7, 5, 10, 2, 11, 12, 99],
    );
    Ok(())
}

#[test]
fn whole_and_nested_overlaps_merge_without_duplicate_bytes() -> TestResult {
    let view = nested_view()?;
    let selected = select(&view).include(|fields| {
        fields.middle
            | fields
                .middle
                .fields(|middle| middle.leaf.fields(|leaf| leaf.body))
    });
    assert_eq!(
        selected.chunks().collect::<Vec<_>>(),
        vec![&[5, 10, 2, 20, 21, 11, 12][..]],
    );
    Ok(())
}

#[test]
fn generic_children_expose_their_concrete_nested_paths() -> TestResult {
    let view = GenericRoot::<Leaf>::view([1, 10, 2, 20, 21, 11])?;
    let selected =
        select(&view).include(|fields| fields.child.fields(|leaf| leaf.head | leaf.tail));
    assert_eq!(selected.bytes().collect::<Vec<_>>(), vec![10, 11]);
    Ok(())
}

mod manual_child {
    use super::*;

    struct ManualLeaf;

    // SAFETY: framing and reconstruction agree on one exact byte; selection exposes that range.
    #[allow(unsafe_code)]
    unsafe impl wire_repr::WireView for ManualLeaf {
        type Error = wire_repr::NeedMore;
        type State = ();
        type View<'view> = &'view [u8];

        const FIXED_SIZE: Option<usize> = Some(1);

        fn frame(
            input: &[u8],
            offset: usize,
        ) -> Result<wire_repr::Frame<Self::State>, Self::Error> {
            if input.is_empty() {
                return Err(wire_repr::NeedMore {
                    offset,
                    additional_at_least: 1,
                });
            }
            Ok(wire_repr::Frame::new((), 1))
        }

        unsafe fn from_validated_parts<'view>(
            input: &'view [u8],
            _state: &'view Self::State,
        ) -> Self::View<'view> {
            input
        }

        unsafe fn selection_field_range(
            _input: &[u8],
            _state: &Self::State,
            index: usize,
        ) -> Option<core::ops::Range<usize>> {
            (index == 0).then_some(0..1)
        }
    }

    struct ManualLeafFields<Prefix: wire_repr::__private::FieldPrefix> {
        byte: wire_repr::__private::FieldPath<
            <Prefix as wire_repr::__private::FieldPrefix>::Append<0>,
        >,
    }

    // SAFETY: the field family preserves Prefix and maps its only ordinal to the exact byte hook.
    #[allow(unsafe_code)]
    unsafe impl wire_repr::__private::WireFieldSchema for ManualLeaf {
        type Fields<Prefix: wire_repr::__private::FieldPrefix> = ManualLeafFields<Prefix>;

        unsafe fn fields<Prefix: wire_repr::__private::FieldPrefix>() -> Self::Fields<Prefix> {
            ManualLeafFields {
                // SAFETY: this path is emitted with the supplied prefix and matching ordinal.
                byte: unsafe { wire_repr::__private::FieldPath::new() },
            }
        }
    }

    #[test]
    fn manual_children_support_whole_and_explicit_nested_selection() -> TestResult {
        let view = GenericRoot::<ManualLeaf>::view([1, 9])?;
        let whole = select(&view).include(|fields| fields.child);
        assert_eq!(whole.bytes().collect::<Vec<_>>(), vec![9]);
        let nested = select(&view).include(|fields| fields.child.fields(|leaf| leaf.byte));
        assert_eq!(nested.bytes().collect::<Vec<_>>(), vec![9]);
        Ok(())
    }
}
