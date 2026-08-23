use super::schema::{DirectSelectionPacket, NestedSelectionChild, NestedSelectionPacket};

/// Selects two reversed fixed fields through generated prepared bytes.
#[inline(never)]
pub(super) fn generated_direct_prepared_selection(
    first: u8,
    skipped: u8,
    last: u8,
    output: &mut [u8; 2],
) {
    DirectSelectionPacket {
        first,
        skipped,
        last,
    }
    .prepare()
    .unwrap()
    .bytes()
    .include(|fields| fields.last | fields.first)
    .write_into(output);
}

/// Selects the same fixed fields directly in their wire order.
#[inline(never)]
pub(super) fn handwritten_direct_prepared_selection(
    first: u8,
    _: u8,
    last: u8,
    output: &mut [u8; 2],
) {
    output[0] = first;
    output[1] = last;
}

/// Selects a nested fixed member through generated translated prepared bytes.
#[inline(never)]
pub(super) fn generated_nested_prepared_selection(
    lead: u8,
    tag: u8,
    member: u16,
    tail: u8,
    output: &mut [u8; 2],
) {
    NestedSelectionPacket {
        lead,
        child: NestedSelectionChild { tag, member },
        tail,
    }
    .prepare()
    .unwrap()
    .bytes()
    .include(|fields| fields.child.member)
    .write_into(output);
}

/// Extracts the same nested member directly.
#[inline(never)]
pub(super) fn handwritten_nested_prepared_selection(
    _: u8,
    _: u8,
    member: u16,
    _: u8,
    output: &mut [u8; 2],
) {
    output.copy_from_slice(&member.to_be_bytes());
}
