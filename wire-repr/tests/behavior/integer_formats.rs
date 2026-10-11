#![allow(dead_code)]

use std::error::Error as _;

use wire_repr::{
    NeedMore, SequenceError, Sleb128Error, Uleb128Error, Vu128Error, WireBuilder, WireView, select,
    wire,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// Literal SLEB examples follow the WebAssembly integer grammar. ZigZag follows
// Protocol Buffers; VU128 follows the revised format in John Millikin's article.
macro_rules! integer_cases {
    ($schema:ident, $test:ident, $physical:ty, [$($encoding:ident),+],
        $(($value:expr, $encoded:expr)),+ $(,)?) => {
        #[derive(WireView, WireBuilder)]
        struct $schema {
            #[wire($($encoding),+)]
            value: $physical,
            tail: u8,
        }

        #[test]
        fn $test() -> TestResult {
            let cases: &[($physical, &[u8])] = &[$(($value, $encoded)),+];
            for &(value, encoded) in cases {
                let mut expected = encoded.to_vec();
                expected.push(0xa7);
                let view = $schema::view(expected.as_slice())?;
                assert_eq!(view.value(), value);
                assert_eq!(view.tail(), 0xa7);
                assert_eq!(view.as_bytes(), expected.as_slice());

                let mut output = [0x55; 20];
                let written = $schema::builder(&mut output[..expected.len()])
                    .value(value)?
                    .tail(0xa7)?
                    .finish()?;
                assert_eq!(written.as_bytes(), expected.as_slice());
            }
            Ok(())
        }
    };
}

integer_cases!(
    SignedByte,
    sleb8_preserves_both_signs_at_the_seven_bit_boundary,
    i8,
    [sleb128],
    (0, &[0x00]),
    (-1, &[0x7f]),
    (1, &[0x01]),
    (-64, &[0x40]),
    (63, &[0x3f]),
    (-65, &[0xbf, 0x7f]),
    (64, &[0xc0, 0x00]),
    (i8::MIN, &[0x80, 0x7f]),
    (i8::MAX, &[0xff, 0x00]),
);
integer_cases!(
    SignedShort,
    sleb16_preserves_sign_extension_in_the_last_group,
    i16,
    [sleb128],
    (-8192, &[0x80, 0x40]),
    (8191, &[0xff, 0x3f]),
    (i16::MIN, &[0x80, 0x80, 0x7e]),
    (i16::MAX, &[0xff, 0xff, 0x01]),
);
integer_cases!(
    SignedWord,
    sleb32_matches_standard_examples_and_extrema,
    i32,
    [sleb128],
    (-624485, &[0x9b, 0xf1, 0x59]),
    (624485, &[0xe5, 0x8e, 0x26]),
    (i32::MIN, &[0x80, 0x80, 0x80, 0x80, 0x78]),
    (i32::MAX, &[0xff, 0xff, 0xff, 0xff, 0x07]),
);
integer_cases!(
    SignedLong,
    sleb64_keeps_the_top_sign_bit,
    i64,
    [sleb128],
    (
        i64::MIN,
        &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x7f]
    ),
    (
        i64::MAX,
        &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00]
    ),
);
integer_cases!(
    SignedWide,
    sleb128_reaches_the_full_signed_range,
    i128,
    [sleb128],
    (
        i128::MIN,
        &[
            0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80,
            0x80, 0x80, 0x80, 0x80, 0x7e,
        ]
    ),
    (
        i128::MAX,
        &[
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff, 0xff, 0x01,
        ]
    ),
);

integer_cases!(
    ZigzagByte,
    zigzag_uleb8_maps_negative_values_without_sign_extension,
    i8,
    [uleb128, zigzag],
    (0, &[0x00]),
    (-1, &[0x01]),
    (1, &[0x02]),
    (-2, &[0x03]),
    (2, &[0x04]),
    (-64, &[0x7f]),
    (64, &[0x80, 0x01]),
    (i8::MIN, &[0xff, 0x01]),
    (i8::MAX, &[0xfe, 0x01]),
);
integer_cases!(
    ZigzagShort,
    zigzag_uleb16_covers_minimum_and_maximum,
    i16,
    [uleb128, zigzag],
    (-8192, &[0xff, 0x7f]),
    (8192, &[0x80, 0x80, 0x01]),
    (i16::MIN, &[0xff, 0xff, 0x03]),
    (i16::MAX, &[0xfe, 0xff, 0x03]),
);
integer_cases!(
    ZigzagWord,
    zigzag_uleb32_keeps_all_transformed_bits,
    i32,
    [uleb128, zigzag],
    (i32::MIN, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
    (i32::MAX, &[0xfe, 0xff, 0xff, 0xff, 0x0f]),
);
integer_cases!(
    ZigzagLong,
    zigzag_uleb64_keeps_all_transformed_bits,
    i64,
    [uleb128, zigzag],
    (
        i64::MIN,
        &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01]
    ),
    (
        i64::MAX,
        &[0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01]
    ),
);
integer_cases!(
    ZigzagWide,
    zigzag_uleb128_handles_minimum_without_negating_it,
    i128,
    [uleb128, zigzag],
    (
        i128::MIN,
        &[
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff, 0xff, 0x03,
        ]
    ),
    (
        i128::MAX,
        &[
            0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff, 0xff, 0x03,
        ]
    ),
);

integer_cases!(
    PrefixByte,
    vu8_uses_the_revised_unary_prefix,
    u8,
    [vu128],
    (0, &[0x00]),
    (127, &[0x7f]),
    (128, &[0x80, 0x02]),
    (u8::MAX, &[0xbf, 0x03]),
);
integer_cases!(
    PrefixShort,
    vu16_crosses_the_fourteen_bit_prefix_boundary,
    u16,
    [vu128],
    (0x3fff, &[0xbf, 0xff]),
    (0x4000, &[0xc0, 0x00, 0x02]),
    (u16::MAX, &[0xdf, 0xff, 0x07]),
);
integer_cases!(
    PrefixWord,
    vu32_matches_revised_standard_examples,
    u32,
    [vu128],
    (0xabcde, &[0xde, 0xe6, 0x55]),
    (0x1fffff, &[0xdf, 0xff, 0xff]),
    (0x200000, &[0xe0, 0x00, 0x00, 0x02]),
    (0x0fffffff, &[0xef, 0xff, 0xff, 0xff]),
    (0x10000000, &[0xf3, 0x00, 0x00, 0x00, 0x10]),
    (0x12345678, &[0xf3, 0x78, 0x56, 0x34, 0x12]),
    (u32::MAX, &[0xf3, 0xff, 0xff, 0xff, 0xff]),
);
integer_cases!(
    PrefixLong,
    vu64_uses_a_binary_length_prefix,
    u64,
    [vu128],
    (
        0xabcdef12_34567890,
        &[0xf7, 0x90, 0x78, 0x56, 0x34, 0x12, 0xef, 0xcd, 0xab]
    ),
    (
        u64::MAX,
        &[0xf7, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
    ),
);
integer_cases!(
    PrefixWide,
    vu128_uses_all_sixteen_payload_bytes,
    u128,
    [vu128],
    (0, &[0x00]),
    (
        1 << 127,
        &[
            0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x80,
        ]
    ),
    (
        u128::MAX,
        &[
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff,
        ]
    ),
);

integer_cases!(
    SignedPrefixByte,
    zigzag_vu8_maps_both_signs_at_the_prefix_boundary,
    i8,
    [vu128, zigzag],
    (0, &[0x00]),
    (-1, &[0x01]),
    (1, &[0x02]),
    (-2, &[0x03]),
    (2, &[0x04]),
    (-64, &[0x7f]),
    (64, &[0x80, 0x02]),
    (-65, &[0x81, 0x02]),
    (i8::MIN, &[0xbf, 0x03]),
    (i8::MAX, &[0xbe, 0x03]),
);
integer_cases!(
    SignedPrefixShort,
    zigzag_vu16_crosses_the_compact_prefix_boundary,
    i16,
    [vu128, zigzag],
    (-8192, &[0xbf, 0xff]),
    (8192, &[0xc0, 0x00, 0x02]),
    (i16::MIN, &[0xdf, 0xff, 0x07]),
    (i16::MAX, &[0xde, 0xff, 0x07]),
);
integer_cases!(
    SignedPrefixWord,
    zigzag_vu32_keeps_the_full_transformed_range,
    i32,
    [vu128, zigzag],
    (i32::MIN, &[0xf3, 0xff, 0xff, 0xff, 0xff]),
    (i32::MAX, &[0xf3, 0xfe, 0xff, 0xff, 0xff]),
);
integer_cases!(
    SignedPrefixLong,
    zigzag_vu64_keeps_the_full_transformed_range,
    i64,
    [vu128, zigzag],
    (
        i64::MIN,
        &[0xf7, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
    ),
    (
        i64::MAX,
        &[0xf7, 0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
    ),
);
integer_cases!(
    SignedPrefixWide,
    zigzag_vu128_handles_the_minimum_without_negating_it,
    i128,
    [vu128, zigzag],
    (
        i128::MIN,
        &[
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff,
        ]
    ),
    (
        i128::MAX,
        &[
            0xff, 0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff,
        ]
    ),
);

#[test]
fn sleb_widths_reject_inconsistent_terminal_sign_bits_and_excess_continuation() {
    macro_rules! overflow {
        ($schema:ident, $error:ident, $bits:literal, $positive:expr, $negative:expr, $continued:expr $(, $edge:expr)*) => {
            let inputs: &[&[u8]] = &[$positive, $negative, $continued $(, $edge)*];
            for &input in inputs {
                assert!(matches!(
                    $schema::view(input),
                    Err($error::ValueSleb128(Sleb128Error::Overflow {
                        offset: 0,
                        bits: $bits
                    }))
                ));
            }
        };
    }
    overflow!(
        SignedByte,
        SignedByteViewError,
        8,
        &[0xff, 0x01],
        &[0x80, 0x7e],
        &[0x80; 2],
        &[0x80, 0x01],
        &[0xff, 0x7e]
    );
    overflow!(
        SignedShort,
        SignedShortViewError,
        16,
        &[0xff, 0xff, 0x02],
        &[0x80, 0x80, 0x7d],
        &[0x80; 3],
        &[0x80, 0x80, 0x02],
        &[0xff, 0xff, 0x7d]
    );
    overflow!(
        SignedWord,
        SignedWordViewError,
        32,
        &[0xff, 0xff, 0xff, 0xff, 0x08],
        &[0x80, 0x80, 0x80, 0x80, 0x77],
        &[0x80; 5],
        &[0x80, 0x80, 0x80, 0x80, 0x08],
        &[0xff, 0xff, 0xff, 0xff, 0x77]
    );
    overflow!(
        SignedLong,
        SignedLongViewError,
        64,
        &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01],
        &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x7e],
        &[0x80; 10],
        &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01],
        &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7e]
    );
    overflow!(
        SignedWide,
        SignedWideViewError,
        128,
        &[
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff, 0xff, 0x02
        ],
        &[
            0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80,
            0x80, 0x80, 0x80, 0x80, 0x7d
        ],
        &[0x80; 19],
        &[
            0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80,
            0x80, 0x80, 0x80, 0x80, 0x02
        ],
        &[
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff, 0xff, 0x7d
        ]
    );
}

#[test]
fn zigzag_uleb_widths_reject_excess_transformed_bits() {
    macro_rules! overflow {
        ($schema:ident, $error:ident, $bits:literal, $encoded:expr) => {
            assert!(matches!(
                $schema::view($encoded),
                Err($error::ValueUleb128(Uleb128Error::Overflow {
                    offset: 0,
                    bits: $bits
                }))
            ));
        };
    }
    overflow!(ZigzagByte, ZigzagByteViewError, 8, [0xff, 0x02]);
    overflow!(ZigzagShort, ZigzagShortViewError, 16, [0xff, 0xff, 0x04]);
    overflow!(
        ZigzagWord,
        ZigzagWordViewError,
        32,
        [0xff, 0xff, 0xff, 0xff, 0x10]
    );
    overflow!(
        ZigzagLong,
        ZigzagLongViewError,
        64,
        [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02]
    );
    overflow!(
        ZigzagWide,
        ZigzagWideViewError,
        128,
        [
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff, 0xff, 0x04
        ]
    );
}

#[test]
fn vu_rejects_value_overflow_without_truncating_a_narrow_target() {
    macro_rules! overflow {
        ($unsigned:ident, $unsigned_error:ident, $signed:ident, $signed_error:ident, $bits:literal, $encoded:expr) => {
            assert!(matches!(
                $unsigned::view($encoded),
                Err($unsigned_error::ValueVu128(Vu128Error::Overflow {
                    offset: 0,
                    bits: $bits
                }))
            ));
            assert!(matches!(
                $signed::view($encoded),
                Err($signed_error::ValueVu128(Vu128Error::Overflow {
                    offset: 0,
                    bits: $bits
                }))
            ));
        };
    }
    overflow!(
        PrefixByte,
        PrefixByteViewError,
        SignedPrefixByte,
        SignedPrefixByteViewError,
        8,
        [0x80, 0x04]
    );
    overflow!(
        PrefixShort,
        PrefixShortViewError,
        SignedPrefixShort,
        SignedPrefixShortViewError,
        16,
        [0xc0, 0x00, 0x08]
    );
    overflow!(
        PrefixWord,
        PrefixWordViewError,
        SignedPrefixWord,
        SignedPrefixWordViewError,
        32,
        [0xf4, 0x00, 0x00, 0x00, 0x00, 0x01]
    );
    overflow!(
        PrefixLong,
        PrefixLongViewError,
        SignedPrefixLong,
        SignedPrefixLongViewError,
        64,
        [0xf8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01]
    );
}

#[derive(WireView)]
struct SignedOffset {
    head: [u8; 3],
    #[wire(sleb128)]
    value: i128,
    tail: u8,
}

#[derive(WireView)]
struct PrefixOffset {
    head: [u8; 3],
    #[wire(vu128)]
    value: u128,
    tail: u8,
}

#[test]
fn sleb_shortage_retains_the_absolute_missing_byte_offset() {
    let mut input = [0x80; 23];
    input[..3].copy_from_slice(&[0x61, 0x62, 0x63]);
    input[21] = 0x7e;
    input[22] = 0xa7;
    for available in 3..22 {
        assert!(matches!(
            SignedOffset::view(&input[..available]),
            Err(SignedOffsetViewError::ValueSleb128(Sleb128Error::NeedMore(NeedMore {
                offset, additional_at_least: 1,
            }))) if offset == available
        ));
    }
    assert!(matches!(
        SignedOffset::view(&input[..22]),
        Err(SignedOffsetViewError::Tail(NeedMore {
            offset: 22,
            additional_at_least: 1
        }))
    ));
}

#[test]
fn vu_shortage_is_exact_after_each_unary_or_binary_prefix_is_known() {
    let cases: &[&[u8]] = &[
        &[0x80, 0x02],
        &[0xc0, 0x00, 0x02],
        &[0xe0, 0x00, 0x00, 0x02],
        &[0xf3, 0x78, 0x56, 0x34, 0x12],
        &[
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff,
        ],
    ];
    for &encoded in cases {
        let mut input = vec![0x61, 0x62, 0x63];
        input.extend_from_slice(encoded);
        input.push(0xa7);
        assert!(matches!(
            PrefixOffset::view(&input[..3]),
            Err(PrefixOffsetViewError::ValueVu128(Vu128Error::NeedMore(
                NeedMore {
                    offset: 3,
                    additional_at_least: 1
                }
            )))
        ));
        for present in 1..encoded.len() {
            let available = 3 + present;
            assert!(matches!(
                PrefixOffset::view(&input[..available]),
                Err(PrefixOffsetViewError::ValueVu128(Vu128Error::NeedMore(NeedMore {
                    offset, additional_at_least,
                }))) if offset == available && additional_at_least == encoded.len() - present
            ));
        }
        assert!(matches!(
            PrefixOffset::view(&input[..input.len() - 1]),
            Err(PrefixOffsetViewError::Tail(NeedMore { offset, additional_at_least: 1 })) if offset == input.len() - 1
        ));
    }
}

#[derive(WireView)]
struct NarrowSignedOffset {
    head: [u8; 3],
    #[wire(sleb128)]
    value: i8,
}

#[derive(WireView)]
struct NarrowPrefixOffset {
    head: [u8; 3],
    #[wire(vu128)]
    value: u8,
}

#[test]
fn codec_sources_remain_typed_and_report_the_absolute_field_start() {
    let signed = match NarrowSignedOffset::view([0x61, 0x62, 0x63, 0xff, 0x01]) {
        Ok(_) => panic!("inconsistent terminal sign bits were accepted"),
        Err(error) => error,
    };
    assert!(matches!(
        signed
            .source()
            .and_then(|source| source.downcast_ref::<Sleb128Error>()),
        Some(Sleb128Error::Overflow { offset: 3, bits: 8 })
    ));
    let prefix = match NarrowPrefixOffset::view([0x61, 0x62, 0x63, 0xf1, 0x00, 0x01]) {
        Ok(_) => panic!("the high payload byte was truncated"),
        Err(error) => error,
    };
    assert!(matches!(
        prefix
            .source()
            .and_then(|source| source.downcast_ref::<Vu128Error>()),
        Some(Vu128Error::Overflow { offset: 3, bits: 8 })
    ));
}

#[derive(WireView, WireBuilder)]
struct MixedIntegers {
    head: u8,
    #[wire(sleb128)]
    signed: i32,
    #[wire(uleb128, zigzag)]
    transformed: i16,
    #[wire(vu128)]
    prefix: u32,
    #[wire(vu128, zigzag)]
    signed_prefix: i64,
    #[wire(be)]
    tail: u16,
}

#[derive(WireBuilder)]
struct IntegerHolder<T> {
    count: u8,
    #[wire(counted_by = count)]
    values: wire::Array<T>,
}

const MIXED_SHORTEST: &[u8] = &[
    0x39, 0x9b, 0xf1, 0x59, 0x81, 0x01, 0xde, 0xe6, 0x55, 0x81, 0x02, 0x14, 0x53,
];
const MIXED_NONMINIMAL: &[u8] = &[
    0x39, 0x9b, 0xf1, 0xd9, 0x7f, 0x81, 0x81, 0x00, 0xf3, 0xde, 0xbc, 0x0a, 0x00, 0xf1, 0x81, 0x00,
    0x14, 0x53,
];

#[test]
fn mixed_nonminimal_values_select_and_forward_the_exact_physical_source() -> TestResult {
    let view = MixedIntegers::view(MIXED_NONMINIMAL)?;
    assert_eq!(view.signed(), -624485);
    assert_eq!(view.transformed(), -65);
    assert_eq!(view.prefix(), 0xabcde);
    assert_eq!(view.signed_prefix(), -65);
    assert_eq!(view.tail(), 0x1453);
    let selected = select(&view).include(|fields| fields.signed | fields.prefix);
    assert_eq!(selected.len(), 9);
    assert_eq!(
        selected.chunks().collect::<Vec<_>>(),
        [&MIXED_NONMINIMAL[1..5], &MIXED_NONMINIMAL[8..13]]
    );
    assert_eq!(
        select(&view)
            .exclude(|fields| fields.transformed | fields.signed_prefix)
            .bytes()
            .collect::<Vec<_>>(),
        [
            0x39, 0x9b, 0xf1, 0xd9, 0x7f, 0xf3, 0xde, 0xbc, 0x0a, 0x00, 0x14, 0x53
        ]
    );

    let mut forwarded = [0x55; 19];
    let written = IntegerHolder::<MixedIntegers>::builder(&mut forwarded[..])
        .values(|values| values.item_view(view))?
        .finish()?;
    assert_eq!(&written.as_bytes()[1..], MIXED_NONMINIMAL);
    let mut fresh = [0x55; 13];
    let written = MixedIntegers::builder(&mut fresh[..])
        .head(0x39)?
        .signed(-624485)?
        .transformed(-65)?
        .prefix(0xabcde)?
        .signed_prefix(-65)?
        .tail(0x1453)?
        .finish()?;
    assert_eq!(written.as_bytes(), MIXED_SHORTEST);
    Ok(())
}

#[test]
fn nonminimal_sign_extension_and_full_vu_prefixes_are_legal_for_narrow_values() -> TestResult {
    assert_eq!(SignedByte::view([0xff, 0x7f, 0xa7])?.value(), -1);
    assert_eq!(SignedByte::view([0x80, 0x00, 0xa7])?.value(), 0);
    assert_eq!(ZigzagByte::view([0x81, 0x00, 0xa7])?.value(), -1);
    let mut full = [0u8; 18];
    full[0] = 0xff;
    full[1] = 0xff;
    full[17] = 0xa7;
    let view = PrefixByte::view(full)?;
    assert_eq!(view.value(), u8::MAX);
    assert_eq!(select(&view).include(|fields| fields.value).len(), 17);
    let mut copy = [0x55; 19];
    let written = IntegerHolder::<PrefixByte>::builder(&mut copy[..])
        .values(|values| values.item_view(view))?
        .finish()?;
    assert_eq!(&written.as_bytes()[1..], &full);
    assert_eq!(SignedPrefixByte::view(full)?.value(), i8::MIN);
    Ok(())
}

#[derive(WireView, WireBuilder)]
struct PrefixCell {
    value: u8,
}

#[derive(WireView, WireBuilder)]
struct PrefixControlled {
    #[wire(vu128)]
    length: u32,
    #[wire(bytes = length)]
    payload: wire::Bytes,
    #[wire(vu128)]
    count: u32,
    marker: u8,
    #[wire(counted_by = count)]
    cells: wire::Array<PrefixCell>,
    #[wire(be)]
    tail: u16,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum PrefixEnvelope {
    #[wire(value = 0x72)]
    Record(PrefixControlled),
}

#[test]
fn vu_byte_and_count_patches_expand_at_all_compact_length_transitions() -> TestResult {
    let cases: &[(usize, usize, &[u8], &[u8])] = &[
        (0, 0, &[0x00], &[0x00]),
        (127, 127, &[0x7f], &[0x7f]),
        (128, 128, &[0x80, 0x02], &[0x80, 0x02]),
        (16383, 16383, &[0xbf, 0xff], &[0xbf, 0xff]),
        (16384, 16384, &[0xc0, 0x00, 0x02], &[0xc0, 0x00, 0x02]),
        (2097151, 1, &[0xdf, 0xff, 0xff], &[0x01]),
        (2097152, 1, &[0xe0, 0x00, 0x00, 0x02], &[0x01]),
    ];
    for &(length, count, byte_prefix, count_prefix) in cases {
        let payload = vec![0x5c; length];
        let mut expected = byte_prefix.to_vec();
        expected.extend_from_slice(&payload);
        expected.extend_from_slice(count_prefix);
        expected.push(0x35);
        expected.extend((0..count).map(|index| index as u8));
        expected.extend_from_slice(&[0x14, 0x53]);

        let mut progressive = vec![0x55; expected.len()];
        let written = PrefixControlled::builder(progressive.as_mut_slice())
            .payload(payload.as_slice())?
            .marker(0x35)?
            .cells(|cells| cells.try_extend(0..count, |cell, index| cell.value(index as u8)))?
            .tail(0x1453)?
            .finish()?;
        assert_eq!(written.as_bytes(), expected.as_slice());
        let view = PrefixControlled::view(progressive.as_slice())?;
        assert_eq!(view.length() as usize, length);
        assert_eq!(view.count() as usize, count);
        assert_eq!(view.payload(), payload.as_slice());
        assert_eq!(view.marker(), 0x35);
        assert_eq!(view.tail(), 0x1453);
        let observed = view
            .cells()
            .iter()
            .map(|cell| cell.map(|cell| cell.view().value()))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            observed,
            (0..count).map(|index| index as u8).collect::<Vec<_>>()
        );
        assert_eq!(
            select(&view)
                .include(|fields| fields.length)
                .bytes()
                .collect::<Vec<_>>(),
            byte_prefix
        );
        assert_eq!(
            select(&view)
                .include(|fields| fields.count)
                .bytes()
                .collect::<Vec<_>>(),
            count_prefix
        );

        expected.insert(0, 0x72);
        let mut detached = vec![0x55; expected.len()];
        let written = PrefixEnvelope::builder(detached.as_mut_slice())
            .record(|record| {
                record
                    .payload(payload.as_slice())
                    .marker(0x35)
                    .cells(|cells| cells.extend(0..count, |cell, index| cell.value(index as u8)))
                    .tail(0x1453)
            })?
            .finish()?;
        assert_eq!(written.as_bytes(), expected.as_slice());
        let envelope = PrefixEnvelope::view(detached.as_slice())?;
        let PrefixEnvelopeVariant::Record(record) = envelope.variant();
        assert_eq!(record.length() as usize, length);
        assert_eq!(record.count() as usize, count);
        assert_eq!(record.payload(), payload.as_slice());
        assert_eq!(record.tail(), 0x1453);
    }
    Ok(())
}

#[derive(WireView)]
struct IntegerCursorHead {
    kind: u8,
}

#[test]
fn mixed_sequences_and_heterogeneous_cursors_consume_exact_integer_lengths() -> TestResult {
    let mut input = MIXED_SHORTEST.to_vec();
    input.extend_from_slice(MIXED_NONMINIMAL);
    let mut views = MixedIntegers::views(input.as_slice())?;
    let first = views.next()?.expect("first mixed representation");
    assert_eq!(first.as_bytes(), MIXED_SHORTEST);
    assert_eq!(views.position(), MIXED_SHORTEST.len());
    let second = views.next()?.expect("second mixed representation");
    assert_eq!(second.as_bytes(), MIXED_NONMINIMAL);
    assert_eq!(second.signed(), first.signed());
    assert_eq!(views.position(), input.len());
    assert!(views.next()?.is_none());

    input.insert(0, 0x61);
    input.extend_from_slice(&[0x80, 0x7f, 0xa7, 0xc0, 0x00, 0x02, 0xa7]);
    let (head, mut cursor) = IntegerCursorHead::cursor(input.as_slice())?;
    let first = MixedIntegers::next(&mut cursor)?;
    let second = MixedIntegers::next(&mut cursor)?;
    let signed = SignedByte::next(&mut cursor)?;
    let prefix = PrefixShort::next(&mut cursor)?;
    assert_eq!(head.kind(), 0x61);
    assert_eq!(first.as_bytes(), MIXED_SHORTEST);
    assert_eq!(second.as_bytes(), MIXED_NONMINIMAL);
    assert_eq!(signed.value(), i8::MIN);
    assert_eq!(prefix.value(), 16384);
    assert_eq!(cursor.position(), input.len());
    assert_eq!(cursor.remaining(), &[]);
    Ok(())
}

#[test]
fn sequence_and_cursor_codec_failures_keep_absolute_offsets_and_do_not_advance() -> TestResult {
    let mut signed_views = SignedByte::views(&[0x00, 0xa7, 0xff, 0x01])?;
    let first = signed_views.next()?.expect("valid first signed item");
    assert_eq!(first.value(), 0);
    assert!(matches!(
        signed_views.next(),
        Err(SequenceError::Schema(SignedByteViewError::ValueSleb128(
            Sleb128Error::Overflow { offset: 2, bits: 8 }
        )))
    ));
    assert_eq!(signed_views.position(), 2);
    assert_eq!(signed_views.remaining(), &[0xff, 0x01]);

    let input = [0x61, 0x81, 0x00, 0xa7, 0xf3, 0x12];
    let (head, mut cursor) = IntegerCursorHead::cursor(&input)?;
    let transformed = ZigzagByte::next(&mut cursor)?;
    assert_eq!(head.kind(), 0x61);
    assert_eq!(transformed.value(), -1);
    assert!(matches!(
        PrefixWord::next(&mut cursor),
        Err(SequenceError::Schema(PrefixWordViewError::ValueVu128(
            Vu128Error::NeedMore(NeedMore {
                offset: 6,
                additional_at_least: 3
            })
        )))
    ));
    assert_eq!(cursor.position(), 4);
    assert_eq!(cursor.remaining(), &[0xf3, 0x12]);
    assert_eq!(transformed.as_bytes(), &input[1..4]);
    Ok(())
}
