//! pair: direct = projection_generated_direct / projection_handwritten_direct
//! pair: nested = projection_generated_nested / projection_handwritten_nested
//! tolerance: 5%
//! weights: instructions=1, branches=8, calls=16, panic_paths=32

use wire_repr::Wire;

#[derive(Wire)]
struct Direct {
    first: u8,
    skipped: u8,
    last: u8,
}
#[derive(Wire)]
struct Child {
    tag: u8,
    #[wire(be)]
    member: u16,
}
#[derive(Wire)]
struct Parent {
    lead: u8,
    child: Child,
    tail: u8,
}

#[inline(never)]
pub fn projection_generated_direct(first: u8, skipped: u8, last: u8, output: &mut [u8; 2]) {
    Direct {
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
#[inline(never)]
pub fn projection_handwritten_direct(first: u8, _: u8, last: u8, output: &mut [u8; 2]) {
    output[0] = first;
    output[1] = last;
}
#[inline(never)]
pub fn projection_generated_nested(lead: u8, tag: u8, member: u16, tail: u8, output: &mut [u8; 2]) {
    Parent {
        lead,
        child: Child { tag, member },
        tail,
    }
    .prepare()
    .unwrap()
    .bytes()
    .include(|fields| fields.child.member)
    .write_into(output);
}
#[inline(never)]
pub fn projection_handwritten_nested(_: u8, _: u8, member: u16, _: u8, output: &mut [u8; 2]) {
    output.copy_from_slice(&member.to_be_bytes());
}

#[test]
fn projection_pairs_are_semantically_equivalent() {
    let mut generated = [0; 2];
    let mut handwritten = [0; 2];
    projection_generated_direct(1, 2, 3, &mut generated);
    projection_handwritten_direct(1, 2, 3, &mut handwritten);
    assert_eq!(generated, handwritten);
    projection_generated_nested(1, 2, 0x0304, 5, &mut generated);
    projection_handwritten_nested(1, 2, 0x0304, 5, &mut handwritten);
    assert_eq!(generated, handwritten);
}
