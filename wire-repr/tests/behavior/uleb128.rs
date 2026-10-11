#![allow(dead_code)]

use core::num::NonZeroU16;

use wire_repr::{
    ConstantMismatch, NeedMore, OutputError, SequenceError, Uleb128Error, WireBuilder, WireView,
    WriteError, output, select, wire,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const U128_MAX_BYTES: &[u8] = &[
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xff, 0xff, 0x03,
];

macro_rules! unsigned_cases {
    ($schema:ident, $test:ident, $physical:ty, $(($value:expr, $encoded:expr)),+ $(,)?) => {
        #[derive(WireView, WireBuilder)]
        struct $schema {
            #[wire(uleb128)]
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

unsigned_cases!(
    ByteVarint,
    byte_values_use_one_or_two_bytes,
    u8,
    (0, &[0x00]),
    (127, &[0x7f]),
    (128, &[0x80, 0x01]),
    (u8::MAX, &[0xff, 0x01]),
);
unsigned_cases!(
    WordVarint,
    word_values_cross_the_fourteen_bit_boundary,
    u16,
    (16383, &[0xff, 0x7f]),
    (16384, &[0x80, 0x80, 0x01]),
    (u16::MAX, &[0xff, 0xff, 0x03]),
);
unsigned_cases!(
    DwordVarint,
    dword_values_keep_all_terminal_bits,
    u32,
    ((1 << 28) - 1, &[0xff, 0xff, 0xff, 0x7f]),
    (1 << 28, &[0x80, 0x80, 0x80, 0x80, 0x01]),
    (u32::MAX, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
);
unsigned_cases!(
    LongVarint,
    long_values_keep_the_highest_unsigned_bit,
    u64,
    (
        1 << 63,
        &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01]
    ),
    (
        u64::MAX,
        &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01]
    ),
);
unsigned_cases!(
    WideVarint,
    wide_values_reach_the_full_u128_range,
    u128,
    (
        1 << 126,
        &[
            0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80,
            0x80, 0x80, 0x80, 0x80, 0x01
        ]
    ),
    (u128::MAX, U128_MAX_BYTES),
);

#[derive(WireView)]
struct WideWithHead {
    head: u8,
    #[wire(uleb128)]
    value: u128,
    tail: u8,
}

#[test]
fn truncation_reports_the_missing_byte_and_the_physical_field() {
    let mut input = vec![0x61];
    input.extend_from_slice(U128_MAX_BYTES);
    input.push(0xa7);

    assert!(matches!(
        WideWithHead::view([]),
        Err(WideWithHeadViewError::Head(NeedMore {
            offset: 0,
            additional_at_least: 1,
        }))
    ));
    for available in 1..=U128_MAX_BYTES.len() {
        assert!(matches!(
            WideWithHead::view(&input[..available]),
            Err(WideWithHeadViewError::ValueUleb128(Uleb128Error::NeedMore(NeedMore {
                offset,
                additional_at_least: 1,
            }))) if offset == available
        ));
    }
    assert!(matches!(
        WideWithHead::view(&input[..input.len() - 1]),
        Err(WideWithHeadViewError::Tail(NeedMore {
            offset: 20,
            additional_at_least: 1,
        }))
    ));
}

#[test]
fn each_unsigned_width_rejects_excess_bits_and_continuation_at_its_limit() {
    macro_rules! overflow {
        ($schema:ident, $error:ident, $bits:literal, $terminal:expr, $continued:expr) => {
            let inputs: [&[u8]; 2] = [$terminal, $continued];
            for input in inputs {
                assert!(matches!(
                    $schema::view(input),
                    Err($error::ValueUleb128(Uleb128Error::Overflow {
                        offset: 0,
                        bits: $bits,
                    }))
                ));
            }
        };
    }

    overflow!(
        ByteVarint,
        ByteVarintViewError,
        8,
        &[0xff, 0x02],
        &[0x80; 2]
    );
    overflow!(
        WordVarint,
        WordVarintViewError,
        16,
        &[0xff, 0xff, 0x04],
        &[0x80; 3]
    );
    overflow!(
        DwordVarint,
        DwordVarintViewError,
        32,
        &[0xff, 0xff, 0xff, 0xff, 0x10],
        &[0x80; 5]
    );
    overflow!(
        LongVarint,
        LongVarintViewError,
        64,
        &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02],
        &[0x80; 10]
    );
    overflow!(
        WideVarint,
        WideVarintViewError,
        128,
        &[
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff, 0xff, 0x04
        ],
        &[0x80; 19]
    );
}

#[derive(WireView, WireBuilder)]
struct LogicalValues {
    #[wire(uleb128, constant = 300)]
    code: u16,
    #[wire(uleb128, as = u16)]
    ordinal: usize,
    #[wire(uleb128, as = u16)]
    nonzero: NonZeroU16,
}

#[test]
fn constants_and_logical_values_use_checked_unsigned_representations() -> TestResult {
    let expected = [0xac, 0x02, 0x80, 0x01, 0x81, 0x01];
    let view = LogicalValues::view(expected)?;
    assert_eq!(view.code(), 300);
    assert_eq!(view.ordinal(), 128);
    assert_eq!(view.nonzero().get(), 129);

    let mut output = [0x55; 6];
    LogicalValues::builder(&mut output[..])
        .ordinal(128)?
        .nonzero(NonZeroU16::new(129).expect("nonzero fixture"))?
        .finish()?;
    assert_eq!(output, expected);

    assert!(matches!(
        LogicalValues::view([0xad, 0x02, 0x80, 0x01, 0x81, 0x01]),
        Err(LogicalValuesViewError::CodeConstant(ConstantMismatch {
            offset: 0,
            expected: 300,
            actual: 301,
        }))
    ));
    assert!(matches!(
        LogicalValues::view([0xac, 0x02, 0x80, 0x01, 0x00]),
        Err(LogicalValuesViewError::NonzeroValue(_))
    ));
    let error = match LogicalValues::builder(&mut output[..]).ordinal(65536) {
        Ok(_) => panic!("out-of-range logical value was encoded"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        WriteError::Schema(LogicalValuesWriteError::OrdinalValue(_))
    ));
    Ok(())
}

#[derive(WireView, WireBuilder)]
struct SelectedValue {
    head: u8,
    #[wire(uleb128)]
    value: u16,
    #[wire(be)]
    tail: u16,
}

#[derive(WireBuilder)]
struct ForwardedValues {
    count: u8,
    #[wire(counted_by = count)]
    values: wire::Array<SelectedValue>,
    end: u8,
}

#[test]
fn nonminimal_values_keep_their_exact_source_range_when_selected_and_forwarded() -> TestResult {
    let source = [0x39, 0xac, 0x82, 0x00, 0x14, 0x53];
    let view = SelectedValue::view(source)?;
    assert_eq!(view.value(), 300);
    assert_eq!(view.tail(), 0x1453);
    let selected = select(&view).include(|fields| fields.value);
    assert_eq!(selected.len(), 3);
    assert_eq!(selected.chunks().collect::<Vec<_>>(), [&source[1..4]]);
    let excluded = select(&view).exclude(|fields| fields.value);
    assert_eq!(excluded.bytes().collect::<Vec<_>>(), [0x39, 0x14, 0x53]);
    assert_eq!(
        excluded.chunks().collect::<Vec<_>>(),
        [&source[..1], &source[4..]]
    );

    let mut forwarded = [0x55; 8];
    ForwardedValues::builder(&mut forwarded[..])
        .values(|values| values.item_view(view))?
        .end(0x91)?
        .finish()?;
    assert_eq!(forwarded, [1, 0x39, 0xac, 0x82, 0x00, 0x14, 0x53, 0x91]);

    let mut fresh = [0x55; 5];
    SelectedValue::builder(&mut fresh[..])
        .head(0x39)?
        .value(300)?
        .tail(0x1453)?
        .finish()?;
    assert_eq!(fresh, [0x39, 0xac, 0x02, 0x14, 0x53]);
    assert_eq!(ByteVarint::view([0x80, 0x00, 0xa7])?.value(), 0);
    assert_eq!(
        DwordVarint::view([0x80, 0x80, 0x80, 0x80, 0x00, 0xa7])?.value(),
        0
    );
    Ok(())
}

#[derive(WireView, WireBuilder)]
struct RawBody {
    #[wire(rest)]
    bytes: wire::Bytes,
}

#[derive(WireView, WireBuilder)]
struct AlignedBody {
    first: [u8; 3],
    #[wire(be, align_before = 4)]
    code: u16,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum InnerKind {
    #[wire(value = 0x42)]
    Raw(RawBody),
    #[wire(value = 0x24)]
    Aligned(AlignedBody),
}

#[derive(WireView, WireBuilder)]
struct BoundedRecord {
    #[wire(uleb128)]
    length: u32,
    #[wire(bytes = length)]
    content: InnerKind,
    #[wire(be, pad_before = 2)]
    tail: u16,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum OuterKind {
    #[wire(value = 0x63)]
    Record(BoundedRecord),
}

#[test]
fn bounded_enum_inside_enum_crosses_both_prefix_growth_boundaries() -> TestResult {
    let cases: &[(usize, &[u8])] = &[
        (127, &[0x7f]),
        (128, &[0x80, 0x01]),
        (16383, &[0xff, 0x7f]),
        (16384, &[0x80, 0x80, 0x01]),
    ];
    for &(length, prefix) in cases {
        let payload = vec![0x5c; length - 1];
        let mut expected = prefix.to_vec();
        expected.push(0x42);
        expected.extend_from_slice(&payload);
        expected.extend_from_slice(&[0, 0, 0x14, 0x53]);

        let mut fixed = vec![0x55; expected.len()];
        let written = BoundedRecord::builder(fixed.as_mut_slice())
            .content(|kind| kind.raw(|body| body.bytes(payload.as_slice())))?
            .tail(0x1453)?
            .finish()?;
        assert_eq!(written.as_bytes(), expected.as_slice());
        let view = BoundedRecord::view(written.as_bytes())?;
        assert_eq!(view.length() as usize, length);
        assert_eq!(view.tail(), 0x1453);
        match view.content().variant() {
            InnerKindVariant::Raw(body) => assert_eq!(body.bytes(), payload.as_slice()),
            InnerKindVariant::Aligned(_) => panic!("wrong inner variant"),
        }

        expected.insert(0, 0x63);
        let mut growing = Vec::new();
        let written = OuterKind::builder(&mut growing)
            .record(|record| {
                record
                    .content(|kind| kind.raw(|body| body.bytes(payload.as_slice())))
                    .tail(0x1453)
            })?
            .finish()?;
        assert_eq!(written.as_bytes(), expected.as_slice());
        let view = OuterKind::view(written.as_bytes())?;
        let OuterKindVariant::Record(record) = view.variant();
        assert_eq!(record.length() as usize, length);
        assert_eq!(record.tail(), 0x1453);
        match record.content().variant() {
            InnerKindVariant::Raw(body) => assert_eq!(body.bytes(), payload.as_slice()),
            InnerKindVariant::Aligned(_) => panic!("wrong nested variant"),
        }
    }
    Ok(())
}

#[derive(WireView, WireBuilder)]
struct AlignedValue {
    #[wire(uleb128)]
    value: u16,
    #[wire(align_before = 4)]
    tail: u8,
}

#[test]
fn known_scalar_alignment_and_child_relative_alignment_remain_sound() -> TestResult {
    let mut scalar = [0x55; 5];
    AlignedValue::builder(&mut scalar[..])
        .value(128)?
        .tail(0x7a)?
        .finish()?;
    assert_eq!(scalar, [0x80, 0x01, 0, 0, 0x7a]);
    assert_eq!(AlignedValue::view(scalar)?.tail(), 0x7a);

    let expected = [0x63, 7, 0x24, 1, 2, 3, 0, 0xab, 0xcd, 0, 0, 0x14, 0x53];
    let mut output = [0x55; 13];
    OuterKind::builder(&mut output[..])
        .record(|record| {
            record
                .content(|kind| kind.aligned(|body| body.first([1, 2, 3]).code(0xabcd)))
                .tail(0x1453)
        })?
        .finish()?;
    assert_eq!(output, expected);
    let view = OuterKind::view(output)?;
    let OuterKindVariant::Record(record) = view.variant();
    match record.content().variant() {
        InnerKindVariant::Aligned(body) => assert_eq!(body.code(), 0xabcd),
        InnerKindVariant::Raw(_) => panic!("wrong aligned variant"),
    }
    Ok(())
}

#[test]
fn prefix_growth_shortage_is_reported_at_finish_without_moving_the_payload() -> TestResult {
    let payload = [0x5c; 127];
    let mut output = [0x55; 135];
    let writer = BoundedRecord::builder(&mut output[1..134])
        .content(|kind| kind.raw(|body| body.bytes(payload)))?
        .tail(0x1453)?;
    let error = match writer.finish() {
        Ok(_) => panic!("prefix growth exceeded exact fixed output"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        WriteError::Output(OutputError::NeedMore {
            request,
            available: 133,
        }) if request.minimum_len == 134
    ));
    assert_eq!(output[0], 0x55);
    assert_eq!(output[134], 0x55);
    assert_eq!(output[2], 0x42);
    assert_eq!(&output[3..130], &payload);
    assert_eq!(&output[130..132], &[0, 0]);
    assert_eq!(&output[132..134], &[0x14, 0x53]);
    Ok(())
}

#[test]
fn detached_child_prefix_growth_preserves_the_parent_selector_and_reports_growth_failure() {
    let payload = [0x5c; 127];
    let mut output = vec![0x55; 134];
    let mut requests = Vec::new();
    let adapter = output::grow_with(&mut output, |_output, request| {
        requests.push(request);
        Err(std::io::Error::other("expansion denied"))
    });
    let error = match OuterKind::builder(adapter).record(|record| {
        record
            .content(|kind| kind.raw(|body| body.bytes(payload)))
            .tail(0x1453)
    }) {
        Ok(_) => panic!("failed prefix expansion was accepted"),
        Err(error) => error,
    };
    assert!(matches!(
        &error,
        WriteError::Output(OutputError::Grow(source)) if source.to_string() == "expansion denied"
    ));
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].high_water_mark, 134);
    assert_eq!(requests[0].minimum_len, 135);
    assert_eq!(output[0], 0x63);
    assert_eq!(output[2], 0x42);
    assert_eq!(&output[3..130], &payload);
    assert_eq!(&output[132..134], &[0x14, 0x53]);
}

#[derive(WireBuilder)]
struct Carried<T> {
    lead: u8,
    body: T,
}

#[derive(WireView, WireBuilder)]
struct SharedSpan {
    #[wire(uleb128)]
    length: u16,
    #[wire(bytes = length)]
    first: wire::Bytes,
    #[wire(bytes = length)]
    second: wire::Bytes,
    tail: u8,
}

#[test]
fn shared_controllers_compare_authoritative_lengths_before_prefix_expansion() -> TestResult {
    let first = [0x31; 128];
    let second = [0x73; 128];
    let mut expected = vec![0x80, 0x01];
    expected.extend_from_slice(&first);
    expected.extend_from_slice(&second);
    expected.push(0x91);
    let mut output = vec![0x55; expected.len()];
    SharedSpan::builder(output.as_mut_slice())
        .first(first)?
        .second(second)?
        .tail(0x91)?
        .finish()?;
    assert_eq!(output, expected);
    let view = SharedSpan::view(output.as_slice())?;
    assert_eq!(view.length(), 128);
    assert_eq!(view.first(), first);
    assert_eq!(view.second(), second);

    let mut detached = Vec::new();
    let written = Carried::<SharedSpan>::builder(&mut detached)
        .lead(0x84)?
        .body(|span| span.first(first).second(second).tail(0x91))?
        .finish()?;
    expected.insert(0, 0x84);
    assert_eq!(written.as_bytes(), expected.as_slice());

    let mut output = [0x55; 300];
    let writer = SharedSpan::builder(&mut output[..]).first(first)?;
    let error = match writer.second([0x73; 129]) {
        Ok(_) => panic!("unequal shared lengths were accepted"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        WriteError::Schema(SharedSpanWriteError::LengthConflict {
            controller: "length",
            expected: 128,
            actual: 129,
        })
    ));
    let error = match Carried::<SharedSpan>::builder(&mut output[..])
        .lead(0x84)?
        .body(|span| span.first(first).second([0x73; 129]).tail(0x91))
    {
        Ok(_) => panic!("unequal detached shared lengths were accepted"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        WriteError::Schema(CarriedWriteError::Body(
            SharedSpanWriteError::LengthConflict {
                controller: "length",
                expected: 128,
                actual: 129,
            }
        ))
    ));
    Ok(())
}

#[derive(WireView, WireBuilder)]
struct SharedNestedSpan {
    #[wire(uleb128)]
    length: u16,
    #[wire(bytes = length)]
    first: RawBody,
    #[wire(bytes = length)]
    second: wire::Bytes,
    tail: u8,
}

#[test]
fn detached_nested_and_raw_payloads_share_one_authoritative_length() -> TestResult {
    let first = [0x31; 128];
    let second = [0x73; 128];
    let mut expected = vec![0x84, 0x80, 0x01];
    expected.extend_from_slice(&first);
    expected.extend_from_slice(&second);
    expected.push(0x91);
    let mut output = vec![0x55; expected.len()];
    let written = Carried::<SharedNestedSpan>::builder(output.as_mut_slice())
        .lead(0x84)?
        .body(|span| {
            span.first(|body| body.bytes(first))
                .second(second)
                .tail(0x91)
        })?
        .finish()?;
    assert_eq!(written.as_bytes(), expected);
    let view = SharedNestedSpan::view(&written.as_bytes()[1..])?;
    assert_eq!(view.length(), 128);
    assert_eq!(view.second(), second);
    assert_eq!(view.tail(), 0x91);

    let mut output = [0x55; 300];
    let error = match Carried::<SharedNestedSpan>::builder(&mut output[..])
        .lead(0x84)?
        .body(|span| {
            span.first(|body| body.bytes(first))
                .second([0x73; 129])
                .tail(0x91)
        }) {
        Ok(_) => panic!("unequal nested and raw lengths were accepted"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        WriteError::Schema(CarriedWriteError::Body(
            SharedNestedSpanWriteError::LengthConflict {
                controller: "length",
                expected: 128,
                actual: 129,
            }
        ))
    ));
    assert_eq!(output[0], 0x84);
    assert_eq!(&output[2..130], first);
    assert!(output[130..].iter().all(|&byte| byte == 0x55));
    Ok(())
}

#[derive(WireView, WireBuilder)]
struct CellValue {
    #[wire(uleb128)]
    value: u16,
}

#[derive(WireView, WireBuilder)]
struct Interleaved {
    #[wire(uleb128)]
    length: u16,
    #[wire(bytes = length)]
    content: InnerKind,
    #[wire(uleb128)]
    count: u16,
    marker: u8,
    #[wire(counted_by = count)]
    cells: wire::Array<CellValue>,
    tail: u8,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum InterleavedKind {
    #[wire(value = 0x71)]
    Row(Interleaved),
}

#[test]
fn multiple_unknown_prefixes_expand_in_reverse_order_on_both_writer_surfaces() -> TestResult {
    let payload = [0x5c; 127];
    let mut expected = vec![0x80, 0x01, 0x42];
    expected.extend_from_slice(&payload);
    expected.extend_from_slice(&[0x80, 0x01, 0x35]);
    for _ in 0..128 {
        expected.extend_from_slice(&[0x81, 0x01]);
    }
    expected.push(0x93);

    let mut fixed = vec![0x55; expected.len()];
    Interleaved::builder(fixed.as_mut_slice())
        .content(|kind| kind.raw(|body| body.bytes(payload)))?
        .marker(0x35)?
        .cells(|cells| cells.try_extend(0..128, |cell, _| cell.value(129)))?
        .tail(0x93)?
        .finish()?;
    assert_eq!(fixed, expected);
    let view = Interleaved::view(fixed.as_slice())?;
    assert_eq!(view.length(), 128);
    assert_eq!(view.count(), 128);
    assert_eq!(view.marker(), 0x35);
    assert_eq!(view.tail(), 0x93);
    let values = view
        .cells()
        .iter()
        .map(|cell| cell.map(|cell| cell.view().value()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(values, [129; 128]);

    expected.insert(0, 0x71);
    let mut fixed = vec![0x55; expected.len()];
    InterleavedKind::builder(fixed.as_mut_slice())
        .row(|row| {
            row.content(|kind| kind.raw(|body| body.bytes(payload)))
                .marker(0x35)
                .cells(|cells| cells.extend(0..128, |cell, _| cell.value(129)))
                .tail(0x93)
        })?
        .finish()?;
    assert_eq!(fixed, expected);
    let view = InterleavedKind::view(fixed.as_slice())?;
    let InterleavedKindVariant::Row(row) = view.variant();
    assert_eq!(row.count(), 128);
    assert_eq!(row.marker(), 0x35);
    assert_eq!(row.tail(), 0x93);

    let mut empty = [0x55; 5];
    Interleaved::builder(&mut empty[..])
        .content(|kind| kind.raw(|body| body.bytes([])))?
        .marker(0x35)?
        .cells(|cells| {
            cells.try_extend(core::iter::empty::<u16>(), |cell, value| cell.value(value))
        })?
        .tail(0x93)?
        .finish()?;
    assert_eq!(empty, [1, 0x42, 0, 0x35, 0x93]);

    let mut detached_empty = [0x55; 6];
    Carried::<Interleaved>::builder(&mut detached_empty[..])
        .lead(0x84)?
        .body(|row| {
            row.content(|kind| kind.raw(|body| body.bytes([])))
                .marker(0x35)
                .cells(|cells| {
                    cells.extend(core::iter::empty::<u16>(), |cell, value| cell.value(value))
                })
                .tail(0x93)
        })?
        .finish()?;
    assert_eq!(detached_empty, [0x84, 1, 0x42, 0, 0x35, 0x93]);
    Ok(())
}

#[derive(WireView, WireBuilder)]
struct ProjectedControllers {
    raw: u8,
    #[wire(uleb128)]
    length: u16,
    #[wire(bytes = length)]
    payload: wire::Bytes,
    #[wire(vu128)]
    count: u16,
    #[wire(counted_by = count)]
    cells: wire::Array<CellValue>,
    #[wire(bits_of = raw, bit = 0)]
    enabled: bool,
    #[wire(bits_of = raw, bits = 1..=3)]
    category: u8,
    tail: u8,
}

#[test]
fn bit_projection_transitions_preserve_deferred_byte_and_count_values() -> TestResult {
    let payload = [0x2d; 128];
    let mut expected = vec![0b1011, 0x80, 0x01];
    expected.extend_from_slice(&payload);
    expected.extend_from_slice(&[0x80, 0x02]);
    for _ in 0..128 {
        expected.extend_from_slice(&[0x81, 0x01]);
    }
    expected.push(0xa3);

    let mut output = vec![0x55; expected.len()];
    let written = ProjectedControllers::builder(output.as_mut_slice())
        .payload(payload)?
        .cells(|cells| cells.try_extend(0..128, |cell, _| cell.value(129)))?
        .enabled(true)?
        .category(5)?
        .tail(0xa3)?
        .finish()?;
    assert_eq!(written.as_bytes(), expected.as_slice());

    let view = ProjectedControllers::view(written.as_bytes())?;
    assert!(view.enabled());
    assert_eq!(view.category(), 5);
    assert_eq!(view.length(), 128);
    assert_eq!(view.payload(), &payload);
    assert_eq!(view.count(), 128);
    assert_eq!(view.tail(), 0xa3);
    Ok(())
}

#[derive(WireView, WireBuilder)]
struct CountAfterBody {
    #[wire(uleb128)]
    length: u16,
    #[wire(bytes = length)]
    content: InnerKind,
    #[wire(be)]
    count: u16,
    marker: u8,
    #[wire(counted_by = count)]
    cells: wire::Array<CellValue>,
    tail: u8,
}

#[test]
fn fixed_count_after_an_expanding_prefix_keeps_its_patch_offset() -> TestResult {
    let payload = [0x5c; 127];
    let mut expected = vec![0x80, 0x01, 0x42];
    expected.extend_from_slice(&payload);
    expected.extend_from_slice(&[0, 2, 0x35, 1, 0x80, 1, 0x93]);
    let mut fixed = vec![0x55; expected.len()];
    CountAfterBody::builder(fixed.as_mut_slice())
        .content(|kind| kind.raw(|body| body.bytes(payload)))?
        .marker(0x35)?
        .cells(|cells| cells.try_extend([1, 128], |cell, value| cell.value(value)))?
        .tail(0x93)?
        .finish()?;
    assert_eq!(fixed, expected);
    let view = CountAfterBody::view(fixed.as_slice())?;
    assert_eq!(view.count(), 2);
    assert_eq!(view.marker(), 0x35);
    assert_eq!(view.tail(), 0x93);

    let mut detached = Vec::new();
    let written = Carried::<CountAfterBody>::builder(&mut detached)
        .lead(0x84)?
        .body(|row| {
            row.content(|kind| kind.raw(|body| body.bytes(payload)))
                .marker(0x35)
                .cells(|cells| cells.extend([1, 128], |cell, value| cell.value(value)))
                .tail(0x93)
        })?
        .finish()?;
    expected.insert(0, 0x84);
    assert_eq!(written.as_bytes(), expected.as_slice());
    Ok(())
}

#[derive(WireView, WireBuilder)]
struct OneByte {
    value: u8,
}

#[derive(WireBuilder)]
struct NarrowCount {
    #[wire(uleb128)]
    count: u8,
    #[wire(counted_by = count)]
    cells: wire::Array<OneByte>,
}

#[test]
fn streamed_count_cannot_exceed_the_declared_unsigned_representation() -> TestResult {
    let mut output = [0x55; 260];
    let error = match NarrowCount::builder(&mut output[..])
        .cells(|cells| cells.try_extend(0..256, |cell, _| cell.value(0x2b)))
        .and_then(|writer| writer.finish())
    {
        Ok(_) => panic!("count outside the physical u8 range was encoded"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        WriteError::Schema(NarrowCountWriteError::Layout(wire_repr::LayoutError {
            field: "count",
        }))
    ));

    let error = match Carried::<NarrowCount>::builder(&mut output[..])
        .lead(0x84)?
        .body(|row| row.cells(|cells| cells.extend(0..256, |cell, _| cell.value(0x2b))))
        .and_then(|writer| writer.finish())
    {
        Ok(_) => panic!("detached count outside the physical u8 range was encoded"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        WriteError::Schema(CarriedWriteError::Body(NarrowCountWriteError::Layout(
            wire_repr::LayoutError { field: "count" }
        )))
    ));
    Ok(())
}

fn byte_sum(selection: impl wire_repr::ByteSelection) -> u16 {
    selection
        .bytes()
        .fold(0u16, |sum, byte| sum.wrapping_add(u16::from(byte)))
}

#[derive(WireView, WireBuilder)]
struct SummedRecord {
    #[wire(be, computed = byte_sum(exclude(self)))]
    checksum: u16,
    #[wire(uleb128)]
    length: u16,
    #[wire(bytes = length)]
    content: InnerKind,
    tail: u8,
}

#[derive(WireView, WireBuilder)]
struct CheckedFrame {
    marker: u8,
    #[wire(be)]
    width: u16,
    #[wire(bytes = width)]
    record: SummedRecord,
    end: u8,
}

#[test]
fn prefix_expansion_precedes_computed_selection_and_preserves_adjacent_parent_fields() -> TestResult
{
    let payload = [1; 127];
    let mut expected = vec![0x01, 0x5b, 0x80, 0x01, 0x42];
    expected.extend_from_slice(&payload);
    expected.push(0x19);
    let mut output = [0x55; 133];
    SummedRecord::builder(&mut output[..])
        .content(|kind| kind.raw(|body| body.bytes(payload)))?
        .tail(0x19)?
        .finish()?;
    assert_eq!(output.as_slice(), expected.as_slice());
    let record = SummedRecord::view(output)?;
    assert_eq!(record.checksum(), 347);
    assert_eq!(record.length(), 128);
    let prefix = select(&record).include(|fields| fields.length);
    assert_eq!(prefix.chunks().collect::<Vec<_>>(), [&[0x80, 0x01][..]]);

    let mut expected_frame = vec![0x6d, 0, 0x85];
    expected_frame.extend_from_slice(&expected);
    expected_frame.push(0x9c);
    let mut output = [0x55; 137];
    CheckedFrame::builder(&mut output[..])
        .marker(0x6d)?
        .record(|record| {
            record
                .content(|kind| kind.raw(|body| body.bytes(payload)))
                .tail(0x19)
        })?
        .end(0x9c)?
        .finish()?;
    assert_eq!(output.as_slice(), expected_frame.as_slice());
    let frame = CheckedFrame::view(output)?;
    assert_eq!(frame.marker(), 0x6d);
    assert_eq!(frame.width(), 133);
    assert_eq!(frame.end(), 0x9c);
    assert_eq!(frame.record().checksum(), 347);
    let nested_prefix =
        select(&frame).include(|fields| fields.record.fields(|record| record.length));
    assert_eq!(
        nested_prefix.chunks().collect::<Vec<_>>(),
        [&[0x80, 0x01][..]]
    );
    Ok(())
}

#[derive(WireView)]
struct CursorHead {
    kind: u8,
}

#[test]
fn variable_views_report_absolute_overflow_without_advancing() -> TestResult {
    let input = [0x80, 0x01, 0xa7, 0xff, 0x02];
    let mut views = ByteVarint::views(&input)?;
    let first = views.next()?.expect("first value");
    assert_eq!(first.value(), 128);
    assert_eq!(first.tail(), 0xa7);
    assert!(matches!(
        views.next(),
        Err(SequenceError::Schema(ByteVarintViewError::ValueUleb128(
            Uleb128Error::Overflow { offset: 3, bits: 8 }
        )))
    ));
    assert_eq!(views.position(), 3);
    assert_eq!(views.remaining(), &input[3..]);
    assert_eq!(first.as_bytes(), &input[..3]);
    Ok(())
}

#[test]
fn heterogeneous_cursor_keeps_prior_views_and_is_failure_atomic_on_truncation() -> TestResult {
    let input = [0x61, 0x80, 0x01, 0xa7, 0xff];
    let (head, mut cursor) = CursorHead::cursor(&input)?;
    let first = ByteVarint::next(&mut cursor)?;
    assert_eq!(head.kind(), 0x61);
    assert_eq!(first.value(), 128);
    assert!(matches!(
        ByteVarint::next(&mut cursor),
        Err(SequenceError::Schema(ByteVarintViewError::ValueUleb128(
            Uleb128Error::NeedMore(NeedMore {
                offset: 5,
                additional_at_least: 1,
            })
        )))
    ));
    assert_eq!(cursor.position(), 4);
    assert_eq!(cursor.remaining(), &[0xff]);
    assert_eq!(first.as_bytes(), &input[1..4]);
    Ok(())
}
