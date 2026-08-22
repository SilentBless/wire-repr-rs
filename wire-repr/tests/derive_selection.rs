#![deny(missing_docs, unsafe_code)]
//! Public generated prepared-byte selection coverage.

use core::mem::{size_of, size_of_val};

use wire_repr::Wire;

/// A plain fixed representation for field selection.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct FixedSelection {
    /// Leading header byte.
    pub header: u8,
    /// Opaque payload bytes.
    pub payload: [u8; 2],
    /// Trailing integrity byte.
    pub checksum: u8,
}

/// A fixed representation with physical padding before its tail.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct PaddedSelection {
    /// Leading byte.
    pub header: u8,
    /// Value after padding and alignment.
    #[wire(be, pad_before = 2, align_before = 4)]
    pub payload: u16,
}

/// A fixed field whose physical offset comes from an earlier source field.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct PositionedSelection {
    /// Absolute offset of `payload`.
    pub payload_offset: u8,
    /// Value at the selected offset.
    #[wire(at = payload_offset, be)]
    pub payload: u16,
}

#[test]
fn views_select_exact_source_bytes_across_fixed_dynamic_and_nested_geometry() {
    let fixed = FixedSelection::view(&[1, 2, 3, 4])
        .without_trailing()
        .unwrap();
    assert_eq!(size_of::<FixedSelectionView<'_>>(), size_of::<&[u8]>());
    let root = fixed.bytes();
    assert_eq!(size_of_val(&root), size_of::<&FixedSelectionView<'_>>());
    let mut fixed_root = [0; 4];
    root.write_into(&mut fixed_root);
    assert_eq!(fixed_root, [1, 2, 3, 4]);
    let selected = fixed
        .bytes()
        .include(|fields| fields.checksum | fields.header);
    assert_eq!(
        size_of_val(&selected),
        size_of::<&FixedSelectionView<'_>>() + size_of::<usize>()
    );
    let mut fixed_selected = [0; 2];
    selected.write_into(&mut fixed_selected);
    assert_eq!(fixed_selected, [1, 4]);

    let positioned = PositionedSelection::view(&[4, 0xa1, 0xb2, 0xc3, 0x12, 0x34])
        .without_trailing()
        .unwrap();
    let mut payload = [0; 2];
    positioned
        .bytes()
        .include(|fields| fields.payload)
        .write_into(&mut payload);
    assert_eq!(payload, [0x12, 0x34]);
    let mut without_payload = [0; 4];
    positioned
        .bytes()
        .exclude(|fields| fields.payload)
        .write_into(&mut without_payload);
    assert_eq!(without_payload, [4, 0xa1, 0xb2, 0xc3]);

    let dynamic = DynamicSelection::view(&[3, 2, 3, 4, 9])
        .without_trailing()
        .unwrap();
    let mut dynamic_selected = [0; 4];
    dynamic
        .bytes()
        .include(|fields| fields.payload | fields.length)
        .write_into(&mut dynamic_selected);
    assert_eq!(dynamic_selected, [3, 2, 3, 4]);

    let nested = SelectionOuter::view(&[1, 2, 0xaa, 0xbb, 9, 3])
        .without_trailing()
        .unwrap();
    let mut leaf = [0; 1];
    nested
        .bytes()
        .include(|fields| fields.outer.inner.leaf)
        .write_into(&mut leaf);
    assert_eq!(leaf, [9]);
    let mut child = [0; 4];
    nested
        .bytes()
        .include(|fields| fields.outer)
        .write_into(&mut child);
    assert_eq!(child, [2, 0xaa, 0xbb, 9]);

    let short = PositionedSelectionParent::view(&[2, 0xee, 2, 0xee, 0x12, 0x34, 9])
        .without_trailing()
        .unwrap();
    let long =
        PositionedSelectionParent::view(&[4, 0xee, 0xdd, 0xcc, 4, 0xaa, 0xbb, 0xcc, 0xab, 0xcd, 7])
            .without_trailing()
            .unwrap();
    let mut short_value = [0; 2];
    short
        .bytes()
        .include(|fields| fields.child.value)
        .write_into(&mut short_value);
    assert_eq!(short_value, [0x12, 0x34]);
    let mut long_value = [0; 2];
    long.bytes()
        .include(|fields| fields.child.value)
        .write_into(&mut long_value);
    assert_eq!(long_value, [0xab, 0xcd]);

    let borrowed = BorrowedSelectionParent::view(&[2, 7, 8])
        .without_trailing()
        .unwrap();
    let mut borrowed_payload = [0; 2];
    borrowed
        .bytes()
        .include(|fields| fields.child.payload)
        .write_into(&mut borrowed_payload);
    assert_eq!(borrowed_payload, [7, 8]);
}

#[test]
fn fixed_plan_selects_fields_in_wire_order_without_trait_imports() {
    let plan = FixedSelection {
        header: 1,
        payload: [2, 3],
        checksum: 4,
    }
    .prepare()
    .unwrap();

    let root = plan.bytes();
    assert_eq!(size_of_val(&root), size_of::<&FixedSelectionPlan<'_>>());
    let mut full = [0; 4];
    root.write_into(&mut full);
    assert_eq!(full, [1, 2, 3, 4]);

    let selected = plan
        .bytes()
        .include(|fields| fields.checksum | fields.header);
    assert_eq!(
        size_of_val(&selected),
        size_of::<&FixedSelectionPlan<'_>>() + size_of::<usize>()
    );
    let mut included = [0; 2];
    selected.write_into(&mut included);
    assert_eq!(included, [1, 4]);

    let omitted = plan.bytes().exclude(|fields| fields.checksum);
    let mut excluded = [0; 3];
    omitted.write_into(&mut excluded);
    assert_eq!(excluded, [1, 2, 3]);
}

#[test]
fn padding_is_not_included_but_survives_exclusion() {
    let plan = PaddedSelection {
        header: 7,
        payload: 0x1234,
    }
    .prepare()
    .unwrap();

    let included = plan.bytes().include(|fields| fields.payload);
    let mut payload = [0; 2];
    included.write_into(&mut payload);
    assert_eq!(payload, [0x12, 0x34]);

    let excluded = plan.bytes().exclude(|fields| fields.payload);
    let mut prefix_and_padding = [0; 4];
    excluded.write_into(&mut prefix_and_padding);
    assert_eq!(prefix_and_padding, [7, 0, 0, 0]);
}

#[test]
fn source_position_marker_uses_each_plan_runtime_geometry() {
    let short = PositionedSelection {
        payload_offset: 2,
        payload: 0x1234,
    }
    .prepare()
    .unwrap();
    let long = PositionedSelection {
        payload_offset: 4,
        payload: 0xabcd,
    }
    .prepare()
    .unwrap();

    let mut short_full = [0; 4];
    short.bytes().write_into(&mut short_full);
    assert_eq!(short_full, [2, 0, 0x12, 0x34]);
    let mut short_payload = [0; 2];
    short
        .bytes()
        .include(|fields| fields.payload)
        .write_into(&mut short_payload);
    assert_eq!(short_payload, [0x12, 0x34]);

    let mut long_full = [0; 6];
    long.bytes().write_into(&mut long_full);
    assert_eq!(long_full, [4, 0, 0, 0, 0xab, 0xcd]);
    let mut long_payload = [0; 2];
    long.bytes()
        .include(|fields| fields.payload)
        .write_into(&mut long_payload);
    assert_eq!(long_payload, [0xab, 0xcd]);
}

/// A dynamic representation with a payload-derived length.
#[derive(Wire)]
pub struct DynamicSelection<'wire> {
    /// Payload length derived from the payload extent.
    pub length: u8,
    /// Borrowed payload.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
    /// Trailing checksum.
    pub checksum: u8,
}

/// A representation with a physical remainder field.
#[derive(Wire)]
pub struct RestSelection<'wire> {
    /// Leading kind.
    pub kind: u8,
    /// Complete remaining physical representation.
    #[wire(rest)]
    pub rest: &'wire [u8],
}

/// Fixed child used as one physical parent field.
#[derive(Wire)]
pub struct SelectionChild {
    /// Child tag.
    pub tag: u8,
    /// Child value.
    #[wire(be)]
    pub value: u16,
}

/// A composed parent whose child must remain a top-level selection span.
#[derive(Wire)]
pub struct NestedSelection {
    /// Parent prefix.
    pub lead: u8,
    /// Complete child representation.
    pub child: SelectionChild,
    /// Parent suffix.
    pub tail: u8,
}

/// A dynamic representation with a stored gap before borrowed bytes.
#[derive(Wire)]
pub struct DynamicPaddedSelection<'wire> {
    /// Payload length derived from the payload extent.
    pub length: u8,
    /// Payload placed after physical padding and alignment.
    #[wire(bytes = length, pad_before = 2, align_before = 4)]
    pub payload: &'wire [u8],
    /// Trailing value.
    pub tail: u8,
}

/// Public child declarations exercise proxy construction across a module boundary.
pub mod public_child {
    use wire_repr::Wire;

    /// A lifetime-bearing child whose generated proxy must not inherit semantic arguments.
    #[derive(Wire)]
    pub struct BorrowedChild<'wire> {
        /// Payload size.
        pub length: u8,
        /// Borrowed payload bytes.
        #[wire(bytes = length)]
        pub payload: &'wire [u8],
    }
}

/// Fixed grandchild for recursive selection translation.
#[derive(Wire)]
pub struct SelectionLeaf {
    /// The selected byte.
    pub leaf: u8,
}

/// Dynamic middle level containing a fixed nested grandchild.
#[derive(Wire)]
pub struct SelectionMiddle<'wire> {
    /// Payload size.
    pub length: u8,
    /// A dynamic prefix.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
    /// Nested fixed leaf.
    pub inner: SelectionLeaf,
}

/// Parent for recursive prepared-plan selection.
#[derive(Wire)]
pub struct SelectionOuter<'wire> {
    /// Parent header.
    pub header: u8,
    /// Nested dynamic representation.
    pub outer: SelectionMiddle<'wire>,
    /// Parent tail.
    pub tail: u8,
}

/// Parent of a public, lifetime-bearing nested type.
#[derive(Wire)]
pub struct BorrowedSelectionParent<'wire> {
    /// Child representation.
    pub child: public_child::BorrowedChild<'wire>,
}

/// Child with a runtime-positioned field for nested coordinate translation.
#[derive(Wire)]
pub struct PositionedSelectionChild {
    /// Absolute offset of `value` within this child.
    pub value_offset: u8,
    /// Value selected through the parent proxy.
    #[wire(at = value_offset, be)]
    pub value: u16,
}

/// Parent with its own gap before a runtime-positioned child.
#[derive(Wire)]
pub struct PositionedSelectionParent {
    /// Absolute offset of `child` within this parent.
    pub child_offset: u8,
    /// Child whose selected member has independent runtime geometry.
    #[wire(at = child_offset)]
    pub child: PositionedSelectionChild,
    /// Parent suffix.
    pub tail: u8,
}

#[test]
fn dynamic_plan_selects_top_level_spans_in_wire_order() {
    let payload = [2, 3, 4];
    let plan = DynamicSelection::builder()
        .payload(&payload)
        .checksum(9)
        .prepare()
        .unwrap();

    let root = plan.bytes();
    assert_eq!(
        size_of_val(&root),
        size_of::<&DynamicSelectionPlan<'_, '_>>()
    );
    let mut full = [0; 5];
    root.write_into(&mut full);
    assert_eq!(full, [3, 2, 3, 4, 9]);

    let selected = plan
        .bytes()
        .include(|fields| fields.payload | fields.length);
    assert_eq!(
        size_of_val(&selected),
        size_of::<&DynamicSelectionPlan<'_, '_>>() + size_of::<usize>()
    );
    let mut included = [0; 4];
    selected.write_into(&mut included);
    assert_eq!(included, [3, 2, 3, 4]);

    let omitted = plan.bytes().exclude(|fields| fields.payload);
    let mut excluded = [0; 2];
    omitted.write_into(&mut excluded);
    assert_eq!(excluded, [3, 9]);
}

#[test]
fn rest_and_nested_fields_select_complete_parent_spans() {
    let bytes = [8, 1, 2, 3];
    let rest = RestSelection {
        kind: 7,
        rest: &bytes[1..],
    }
    .prepare()
    .unwrap();
    let mut selected_rest = [0; 3];
    rest.bytes()
        .include(|fields| fields.rest)
        .write_into(&mut selected_rest);
    assert_eq!(selected_rest, [1, 2, 3]);

    let nested = NestedSelection {
        lead: 1,
        child: SelectionChild {
            tag: 2,
            value: 0x0304,
        },
        tail: 5,
    }
    .prepare()
    .unwrap();
    let mut selected_child_and_tail = [0; 4];
    nested
        .bytes()
        .include(|fields| fields.tail | fields.child)
        .write_into(&mut selected_child_and_tail);
    assert_eq!(selected_child_and_tail, [2, 3, 4, 5]);
}

#[test]
fn dynamic_gaps_are_excluded_from_field_spans_but_preserved_by_exclude() {
    let payload = [0xaa, 0xbb];
    let plan = DynamicPaddedSelection::builder()
        .payload(&payload)
        .tail(9)
        .prepare()
        .unwrap();

    let mut selected = [0; 2];
    plan.bytes()
        .include(|fields| fields.payload)
        .write_into(&mut selected);
    assert_eq!(selected, payload);

    let mut omitted = [0; 5];
    plan.bytes()
        .exclude(|fields| fields.payload)
        .write_into(&mut omitted);
    assert_eq!(omitted, [2, 0, 0, 0, 9]);
}

#[test]
fn nested_member_selection_preserves_whole_child_and_wire_order() {
    let plan = NestedSelection {
        lead: 1,
        child: SelectionChild {
            tag: 2,
            value: 0x0304,
        },
        tail: 5,
    }
    .prepare()
    .unwrap();

    let mut member = [0; 2];
    plan.bytes()
        .include(|fields| fields.child.value)
        .write_into(&mut member);
    assert_eq!(member, [3, 4]);

    let mut ordered = [0; 4];
    plan.bytes()
        .include(|fields| fields.tail | fields.child.value | fields.lead)
        .write_into(&mut ordered);
    assert_eq!(ordered, [1, 3, 4, 5]);
}

#[test]
fn recursive_nested_selection_translates_dynamic_child_geometry() {
    let payload = [0xaa, 0xbb];
    let plan = SelectionOuter {
        header: 1,
        outer: SelectionMiddle {
            payload: &payload,
            length: 2,
            inner: SelectionLeaf { leaf: 9 },
        },
        tail: 3,
    }
    .prepare()
    .unwrap();

    let mut selected = [0; 1];
    plan.bytes()
        .include(|fields| fields.outer.inner.leaf)
        .write_into(&mut selected);
    assert_eq!(selected, [9]);

    let mut omitted = [0; 5];
    plan.bytes()
        .exclude(|fields| fields.outer.inner.leaf)
        .write_into(&mut omitted);
    assert_eq!(omitted, [1, 2, 0xaa, 0xbb, 3]);
}

#[test]
fn lifetime_bearing_nested_child_proxy_compiles_and_selects() {
    let payload = [7, 8];
    let plan = BorrowedSelectionParent {
        child: public_child::BorrowedChild {
            payload: &payload,
            length: 2,
        },
    }
    .prepare()
    .unwrap();

    let mut selected = [0; 2];
    plan.bytes()
        .include(|fields| fields.child.payload)
        .write_into(&mut selected);
    assert_eq!(selected, payload);
}

#[test]
fn nested_marker_uses_parent_and_child_runtime_geometry() {
    let short = PositionedSelectionParent {
        child_offset: 2,
        child: PositionedSelectionChild {
            value_offset: 2,
            value: 0x1234,
        },
        tail: 9,
    }
    .prepare()
    .unwrap();
    let long = PositionedSelectionParent {
        child_offset: 4,
        child: PositionedSelectionChild {
            value_offset: 4,
            value: 0xabcd,
        },
        tail: 7,
    }
    .prepare()
    .unwrap();

    let mut short_value = [0; 2];
    short
        .bytes()
        .include(|fields| fields.child.value)
        .write_into(&mut short_value);
    assert_eq!(short_value, [0x12, 0x34]);

    let mut long_value = [0; 2];
    long.bytes()
        .include(|fields| fields.child.value)
        .write_into(&mut long_value);
    assert_eq!(long_value, [0xab, 0xcd]);

    let mut long_without_value = [0; 9];
    long.bytes()
        .exclude(|fields| fields.child.value)
        .write_into(&mut long_without_value);
    assert_eq!(long_without_value, [4, 0, 0, 0, 4, 0, 0, 0, 7]);
}
