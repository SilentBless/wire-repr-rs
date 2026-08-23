use super::*;

#[test]
fn gat_values_and_plans_can_borrow() {
    let input = [0xca, 0xfe];
    let value = Borrowing::decode(&input);
    assert_eq!(Borrowing::plan(value).map(render_plan::<2>), Ok(input));
}

#[test]
fn signed_integer_codecs_round_trip_negative_values_and_endianness() {
    macro_rules! signed_cases {
        ($(($codec:ty, $value:expr, $bytes:expr)),+ $(,)?) => {{
            $(
                let bytes = $bytes;
                assert_eq!(<$codec>::decode(&bytes), $value);
                assert_eq!(
                    <$codec>::plan($value).map(render_plan::<{ <$codec>::WIDTH }>),
                    Ok(bytes),
                );
            )+
        }};
    }

    signed_cases!(
        (I8, -1, [0xff]),
        (BeI16, -0x1234, [0xed, 0xcc]),
        (LeI16, -0x1234, [0xcc, 0xed]),
        (BeI32, -0x0123_4567, [0xfe, 0xdc, 0xba, 0x99]),
        (LeI32, -0x0123_4567, [0x99, 0xba, 0xdc, 0xfe]),
        (
            BeI64,
            -0x0123_4567_89ab_cdef,
            [0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x11]
        ),
        (
            LeI64,
            -0x0123_4567_89ab_cdef,
            [0x11, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe]
        ),
        (
            BeI128,
            -0x0011_2233_4455_6677_8899_aabb_ccdd_eeff,
            [
                0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22,
                0x11, 0x01,
            ]
        ),
        (
            LeI128,
            -0x0011_2233_4455_6677_8899_aabb_ccdd_eeff,
            [
                0x01, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff,
            ]
        ),
    );
}

#[test]
fn u24_codecs_cover_zero_maximum_and_rejection() {
    assert_eq!(BeU24::WIDTH, 3);
    assert_eq!(LeU24::WIDTH, 3);
    assert_eq!(BeU24::decode(&[0x12, 0x34, 0x56]), 0x12_3456);
    assert_eq!(LeU24::decode(&[0x56, 0x34, 0x12]), 0x12_3456);
    assert_eq!(BeU24::plan(0).map(render_plan::<3>), Ok([0, 0, 0]));
    assert_eq!(LeU24::plan(0).map(render_plan::<3>), Ok([0, 0, 0]));
    assert_eq!(
        BeU24::plan(0x00ff_ffff).map(render_plan::<3>),
        Ok([0xff, 0xff, 0xff])
    );
    assert_eq!(
        LeU24::plan(0x00ff_ffff).map(render_plan::<3>),
        Ok([0xff, 0xff, 0xff])
    );
    assert_eq!(
        BeU24::plan(0x0100_0000),
        Err(U24RangeError::new(0x0100_0000))
    );
    let error = U24RangeError::new(0x0100_0000);
    assert_eq!(error.value(), 0x0100_0000);
    assert_eq!(
        error.to_string(),
        "16777216 does not fit in an unsigned 24-bit integer"
    );
}

#[test]
fn unsigned_integer_codecs_round_trip_boundaries_and_endianness() {
    macro_rules! unsigned_cases {
        ($(($codec:ty, $value:expr, $bytes:expr)),+ $(,)?) => {{
            $(
                let bytes = $bytes;
                assert_eq!(<$codec>::decode(&bytes), $value);
                assert_eq!(
                    <$codec>::plan($value).map(render_plan::<{ <$codec>::WIDTH }>),
                    Ok(bytes),
                );
            )+
        }};
    }

    unsigned_cases!(
        (U8, 0xff, [0xff]),
        (BeU16, 0x1234, [0x12, 0x34]),
        (LeU16, 0x1234, [0x34, 0x12]),
        (BeU32, 0x1234_5678, [0x12, 0x34, 0x56, 0x78]),
        (LeU32, 0x1234_5678, [0x78, 0x56, 0x34, 0x12]),
        (
            BeU64,
            0x0123_4567_89ab_cdef,
            [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]
        ),
        (
            LeU64,
            0x0123_4567_89ab_cdef,
            [0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01]
        ),
        (
            BeU128,
            0x0011_2233_4455_6677_8899_aabb_ccdd_eeff,
            [
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff,
            ]
        ),
        (
            LeU128,
            0x0011_2233_4455_6677_8899_aabb_ccdd_eeff,
            [
                0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22,
                0x11, 0x00,
            ]
        ),
    );
}
