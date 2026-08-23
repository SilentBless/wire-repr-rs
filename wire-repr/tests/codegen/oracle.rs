use core::{hint::black_box, mem::size_of_val};
use wire_repr::PreparedLayout;

use super::{
    decode::*,
    encode::*,
    schema::{ComputedPacket, TableChoice, TinyTable, WidePlanPacket},
    selection::*,
};

#[test]
fn generated_probes_match_handwritten_safe_rust() {
    for bytes in [&[][..], &[0x12], &[0x12, 0x34], &[0x12, 0x34, 0]] {
        assert_eq!(
            generated_fixed_decode(black_box(bytes)),
            handwritten_fixed_decode(black_box(bytes))
        );
    }
    for bytes in [&[][..], &[2, 0xaa, 0xbb, 0x55], &[0, 0x55], &[1, 0xaa]] {
        assert_eq!(
            generated_bounded_decode(black_box(bytes)),
            handwritten_bounded_decode(black_box(bytes))
        );
    }
    for bytes in [&[][..], &[1], &[2, 0x12, 0x34], &[3]] {
        assert_eq!(
            generated_enum_decode(black_box(bytes)),
            handwritten_enum_decode(black_box(bytes))
        );
    }

    for bytes in [
        &[][..],
        b"HALT" as &[u8],
        b"DATA\x12\x34" as &[u8],
        b"NOPE" as &[u8],
    ] {
        assert_eq!(
            generated_byte_enum_decode(black_box(bytes)),
            handwritten_byte_enum_decode(black_box(bytes))
        );
    }

    for bytes in [&[][..], &[0x00], &[0x00, 0x0b], &[0xff, 0xff]] {
        assert_eq!(
            generated_bitfield_decode(black_box(bytes)),
            handwritten_bitfield_decode(black_box(bytes))
        );
    }
    for bytes in [
        &[][..],
        &[0x12][..],
        &[0x12, 0x34][..],
        &[0x12, 0x34, 0xab, 0xcd][..],
    ] {
        assert_eq!(
            generated_fixed_sequence(black_box(bytes)),
            handwritten_fixed_sequence(black_box(bytes))
        );
    }
    for bytes in [
        &[][..],
        &[0, 1][..],
        &[1, 9, 2][..],
        &[1, 9, 2, 0, 3][..],
        &[2, 9][..],
    ] {
        assert_eq!(
            generated_variable_cursor(black_box(bytes)),
            handwritten_variable_cursor(black_box(bytes))
        );
    }

    for bytes in [&[][..], &[1, 0], &[1, 7], &[2], &[2, 1], &[3]] {
        assert_eq!(
            generated_validated_enum_decode(black_box(bytes)),
            handwritten_validated_enum_decode(black_box(bytes))
        );
    }

    let mut generated_direct = [0; 2];
    let mut handwritten_direct = [0; 2];
    generated_direct_prepared_selection(
        black_box(1),
        black_box(2),
        black_box(3),
        &mut generated_direct,
    );
    handwritten_direct_prepared_selection(
        black_box(1),
        black_box(2),
        black_box(3),
        &mut handwritten_direct,
    );
    assert_eq!(generated_direct, handwritten_direct);

    let mut generated_nested = [0; 2];
    let mut handwritten_nested = [0; 2];
    generated_nested_prepared_selection(
        black_box(1),
        black_box(2),
        black_box(0x0304),
        black_box(5),
        &mut generated_nested,
    );
    handwritten_nested_prepared_selection(
        black_box(1),
        black_box(2),
        black_box(0x0304),
        black_box(5),
        &mut handwritten_nested,
    );
    assert_eq!(generated_nested, handwritten_nested);
    for bytes in [&[][..], &[0x41, 0x12, 0x34], &[0x7f], &[0x41], &[0x55]] {
        assert_eq!(
            generated_table_decode(black_box(bytes)),
            handwritten_table_decode(black_box(bytes))
        );
    }

    for (payload, length) in [(&[][..], 0), (&[4, 5][..], 3), (&[4, 5][..], 4)] {
        let mut generated = [0xa5; 8];
        let mut handwritten = generated;
        let generated_len = generated_computed_encode(payload, black_box(&mut generated[..length]));
        let handwritten_len =
            handwritten_computed_encode(payload, black_box(&mut handwritten[..length]));
        assert_eq!(generated_len, handwritten_len);
        assert_eq!(generated, handwritten);
    }

    for length in [0, 2, 3, 6, 8] {
        let mut generated = [0xa5; 8];
        let mut handwritten = generated;
        let generated_len = generated_fixed_encode(0x1234, black_box(&mut generated[..length]));
        let handwritten_len =
            handwritten_fixed_encode(0x1234, black_box(&mut handwritten[..length]));
        assert_eq!(generated_len, handwritten_len);
        assert_eq!(generated, handwritten);
        generated.fill(0xa5);
        handwritten.fill(0xa5);
        let generated_len =
            generated_positioned_encode(0x1234, black_box(&mut generated[..length]));
        let handwritten_len =
            handwritten_positioned_encode(0x1234, black_box(&mut handwritten[..length]));
        assert_eq!(generated_len, handwritten_len);
        assert_eq!(generated, handwritten);
    }
}

#[test]
fn generated_request_and_plan_layouts_stay_bounded() {
    let table = TinyTable {
        data: 0x41,
        halt: 0x7f,
    };
    let request = TableChoice::view(&[0x7f]).table(&table);
    assert!(size_of_val(&request) <= 32);
    let view = {
        let table = TinyTable {
            data: 0x41,
            halt: 0x7f,
        };
        TableChoice::view(&[0x41, 0x12, 0x34])
            .table(&table)
            .without_trailing()
            .unwrap()
    };
    assert_eq!(view.data().unwrap().value(), 0x1234);

    let operation_plan = {
        let table = TinyTable {
            data: 0x41,
            halt: 0x7f,
        };
        TableChoice::Halt.table(&table).prepare().unwrap()
    };
    assert!(size_of_val(&operation_plan) <= 32);
    let mut operation_output = [0; 1];
    assert_eq!(
        operation_plan
            .commit_into(&mut operation_output)
            .unwrap()
            .0
            .as_bytes(),
        &[0x7f]
    );

    let computed_builder = ComputedPacket::builder().kind(9).payload(&[1, 2]);
    assert!(size_of_val(&computed_builder) <= 32);
    let computed_plan = computed_builder.prepare().unwrap();
    assert!(size_of_val(&computed_plan) <= 48);

    let wide_plan = WidePlanPacket {
        a: 1,
        b: 2,
        c: 3,
        d: 4,
        e: 5,
        f: 6,
        g: 7,
        h: 8,
        i: 9,
        j: 10,
        k: 11,
        l: 12,
    }
    .prepare()
    .unwrap();
    assert!(size_of_val(&wide_plan) <= 64);
}
