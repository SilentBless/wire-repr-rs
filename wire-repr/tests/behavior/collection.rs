#![allow(dead_code)]

use wire_repr::{WireBuilder, WireView, select};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(WireView, WireBuilder)]
struct Bar {
    #[wire(be)]
    value: u16,
}

#[derive(WireView, WireBuilder)]
struct ConstantBar {
    #[wire(constant = 1)]
    tag: u8,
    value: u8,
}

#[derive(WireView, WireBuilder)]
struct Foo<T> {
    count: u8,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<T>,
    tail: u8,
}

#[derive(WireView, WireBuilder)]
pub struct Entry {
    #[wire(be)]
    key: i32,
    #[wire(be)]
    value: i32,
}

#[derive(WireView, WireBuilder)]
pub struct Batch {
    #[wire(be)]
    count: i32,
    #[wire(counted_by = count)]
    entries: wire_repr::wire::Array<Entry>,
    #[wire(as = u8)]
    enabled: bool,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u16, be)]
pub enum Envelope {
    #[wire(value = 0x3102)]
    Batch(Batch),
}

const BATCH_BYTES: [u8; 23] = [
    0x31, 0x02, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x11, 0xff, 0xff, 0xff, 0xe9, 0x00, 0x00,
    0x00, 0x29, 0xff, 0xff, 0xff, 0xc5, 0x01,
];

#[test]
fn enum_body_streams_detached_entries_into_fixed_and_growing_outputs() -> TestResult {
    let mut fixed = [0u8; 23];
    let written = Envelope::builder(&mut fixed[..])
        .batch(|batch| {
            batch
                .entries(|entries| {
                    entries.extend([(17, -23), (41, -59)], |entry, (key, value)| {
                        entry.key(key).value(value)
                    })
                })
                .enabled(true)
        })?
        .finish()?;
    assert_eq!(written.as_bytes(), BATCH_BYTES);
    assert_eq!(fixed, BATCH_BYTES);

    let view = Envelope::view(fixed)?;
    let EnvelopeVariant::Batch(batch) = view.variant();
    let count: i32 = batch.count();
    assert_eq!(count, 2);
    assert_eq!(view.as_bytes(), BATCH_BYTES);
    assert!(batch.enabled());
    assert_eq!(batch.entries().len(), 2);
    let mut entries = batch.entries().iter();
    let first = entries.next().transpose()?.expect("first entry");
    assert_eq!((first.view().key(), first.view().value()), (17, -23));
    let second = entries.next().transpose()?.expect("second entry");
    assert_eq!((second.view().key(), second.view().value()), (41, -59));
    assert!(entries.next().is_none());

    let mut growing = Vec::new();
    let written = Envelope::builder(&mut growing)
        .batch(|batch| {
            batch
                .entries(|entries| {
                    entries.extend([(17, -23), (41, -59)], |entry, (key, value)| {
                        entry.key(key).value(value)
                    })
                })
                .enabled(true)
        })?
        .finish()?;
    assert_eq!(written.as_bytes(), BATCH_BYTES);
    Ok(())
}

#[test]
fn empty_detached_array_patches_zero_count_and_preserves_the_enum_body_tail() -> TestResult {
    let mut output = [0xff; 7];
    Envelope::builder(&mut output[..])
        .batch(|batch| {
            batch
                .entries(|entries| {
                    entries.extend(core::iter::empty::<(i32, i32)>(), |entry, (key, value)| {
                        entry.key(key).value(value)
                    })
                })
                .enabled(false)
        })?
        .finish()?;
    assert_eq!(output, [0x31, 0x02, 0, 0, 0, 0, 0]);
    let view = Envelope::view(output)?;
    let EnvelopeVariant::Batch(batch) = view.variant();
    let count: i32 = batch.count();
    assert_eq!(count, 0);
    assert_eq!(view.as_bytes(), [0x31, 0x02, 0, 0, 0, 0, 0]);
    assert!(!batch.enabled());
    assert!(batch.entries().is_empty());
    assert!(batch.entries().iter().next().is_none());
    Ok(())
}

#[test]
fn signed_count_sequence_and_cursor_errors_keep_absolute_offsets_and_position() -> TestResult {
    for count in [-1i32, i32::MIN] {
        let mut input = [0u8; 9];
        input[5..].copy_from_slice(&count.to_be_bytes());
        let mut views = Batch::views(&input)?;
        let first = views.next()?.expect("empty first batch");
        assert_eq!(first.count(), 0);
        assert!(matches!(
            views.next(),
            Err(wire_repr::SequenceError::Schema(
                BatchViewError::CountValue(wire_repr::ScalarConversionError {
                    offset: 5,
                    from: "i32",
                    to: "usize",
                })
            ))
        ));
        assert_eq!(views.position(), 5);
        assert_eq!(views.remaining(), &input[5..]);

        let (first, mut cursor) = Batch::cursor(&input)?;
        assert_eq!(first.count(), 0);
        assert!(matches!(
            Batch::next(&mut cursor),
            Err(wire_repr::SequenceError::Schema(
                BatchViewError::CountValue(wire_repr::ScalarConversionError {
                    offset: 5,
                    from: "i32",
                    to: "usize",
                })
            ))
        ));
        assert_eq!(cursor.position(), 5);
        assert_eq!(cursor.remaining(), &input[5..]);
    }
    Ok(())
}

macro_rules! signed_count_cases {
    ($module:ident, $logical:ty, $physical:ty, [$($attribute:meta),* $(,)?],
        $positive_cases:expr, $negative_cases:expr) => {
        mod $module {
            use super::*;

            #[derive(WireView, WireBuilder)]
            pub(super) struct Counted {
                marker: u8,
                $(#[$attribute])*
                count: $logical,
                #[wire(counted_by = count)]
                items: wire_repr::wire::Array<Bar>,
                tail: u8,
            }

            #[derive(WireView, WireBuilder)]
            #[wire(selector = u16, be)]
            pub(super) enum Envelope {
                #[wire(value = 0x6a04)]
                Body(Counted),
            }

            #[test]
            fn reads_and_patches_progressive_detached_and_copied_counts() -> TestResult {
                let cases: &[(usize, &[u8])] = $positive_cases;
                for &(count, prefix) in cases {
                    let values = (0..count)
                        .map(|index| 0x2100 + index as u16)
                        .collect::<Vec<_>>();
                    let mut expected = vec![0x91];
                    expected.extend_from_slice(prefix);
                    for value in &values {
                        expected.extend_from_slice(&value.to_be_bytes());
                    }
                    expected.push(0xa7);

                    let view = Counted::view(expected.as_slice())?;
                    let decoded_count: $logical = view.count();
                    assert_eq!(decoded_count, count as $logical);
                    assert_eq!(view.marker(), 0x91);
                    assert_eq!(view.tail(), 0xa7);
                    assert_eq!(
                        view.items()
                            .iter()
                            .map(|item| item.map(|item| item.view().value()))
                            .collect::<Result<Vec<_>, _>>()?,
                        values
                    );

                    let mut output = vec![0xff; expected.len()];
                    let written = Counted::builder(&mut output[..])
                        .marker(0x91)?
                        .items(|items| {
                            items.try_extend(values.iter().copied(), |bar, value| bar.value(value))
                        })?
                        .tail(0xa7)?
                        .finish()?;
                    assert_eq!(written.as_bytes(), expected);

                    let mut copied_expected = expected.clone();
                    copied_expected[0] = 0x92;
                    *copied_expected.last_mut().expect("tail") = 0x5c;
                    let mut copied = vec![0xff; copied_expected.len()];
                    let written = Counted::builder(&mut copied[..])
                        .marker(0x92)?
                        .items(|items| items.copy_from(view.items()))?
                        .tail(0x5c)?
                        .finish()?;
                    assert_eq!(written.as_bytes(), copied_expected);

                    let mut nested_expected = vec![0x6a, 0x04];
                    nested_expected.extend_from_slice(&expected);
                    let mut nested = vec![0xff; nested_expected.len()];
                    let written = Envelope::builder(&mut nested[..])
                        .body(|body| {
                            body.marker(0x91)
                                .items(|items| {
                                    items.extend(values.iter().copied(), |bar, value| bar.value(value))
                                })
                                .tail(0xa7)
                        })?
                        .finish()?;
                    assert_eq!(written.as_bytes(), nested_expected);
                    let view = Envelope::view(written.as_bytes())?;
                    let EnvelopeVariant::Body(body) = view.variant();
                    let decoded_count: $logical = body.count();
                    assert_eq!(decoded_count, count as $logical);
                    assert_eq!(body.tail(), 0xa7);
                }
                Ok(())
            }

            #[test]
            fn rejects_negative_counts_before_any_missing_item_even_unchecked() {
                let cases: Vec<(Vec<u8>, &str)> = $negative_cases;
                for (prefix, target) in cases {
                    let mut input = vec![0x91];
                    input.extend_from_slice(&prefix);
                    for error in [
                        Counted::view(&input).err().expect("negative count"),
                        Counted::view_unchecked(&input)
                            .err()
                            .expect("unchecked negative count"),
                    ] {
                        assert!(matches!(
                            error,
                            CountedViewError::CountValue(wire_repr::ScalarConversionError {
                                offset: 1,
                                from,
                                to,
                            }) if from == stringify!($physical) && to == target
                        ));
                    }

                    let mut nested = vec![0x6a, 0x04];
                    nested.extend_from_slice(&input);
                    for error in [
                        Envelope::view(&nested).err().expect("nested negative count"),
                        Envelope::view_unchecked(&nested)
                            .err()
                            .expect("unchecked nested negative count"),
                    ] {
                        assert!(matches!(
                            error,
                            EnvelopeViewError::VariantBody(CountedViewError::CountValue(
                                wire_repr::ScalarConversionError {
                                    offset: 3,
                                    from,
                                    to,
                                }
                            )) if from == stringify!($physical) && to == target
                        ));
                    }
                }
            }
        }
    };
}

macro_rules! fixed_signed_count_cases {
    ($module:ident, $logical:ty, $physical:ty, $encode:ident, [$($attribute:meta),*]) => {
        signed_count_cases!(
            $module, $logical, $physical, [$($attribute),*],
            &[(0, &(0 as $physical).$encode()), (2, &(2 as $physical).$encode())],
            {
                let minimum = <$logical>::MIN as $physical;
                let mut cases = vec![
                    ((-1 as $physical).$encode().to_vec(), "usize"),
                    (minimum.$encode().to_vec(), "usize"),
                ];
                if minimum != <$physical>::MIN {
                    cases.push((<$physical>::MIN.$encode().to_vec(), stringify!($logical)));
                }
                cases
            }
        );
    };
}

fixed_signed_count_cases!(fixed_i8, i8, i8, to_be_bytes, []);
fixed_signed_count_cases!(fixed_i16_be, i16, i16, to_be_bytes, [wire(be)]);
fixed_signed_count_cases!(fixed_i16_le, i16, i16, to_le_bytes, [wire(le)]);
fixed_signed_count_cases!(fixed_i32_be, i32, i32, to_be_bytes, [wire(be)]);
fixed_signed_count_cases!(fixed_i32_le, i32, i32, to_le_bytes, [wire(le)]);
fixed_signed_count_cases!(fixed_i64_be, i64, i64, to_be_bytes, [wire(be)]);
fixed_signed_count_cases!(fixed_i64_le, i64, i64, to_le_bytes, [wire(le)]);
fixed_signed_count_cases!(fixed_i128_be, i128, i128, to_be_bytes, [wire(be)]);
fixed_signed_count_cases!(fixed_i128_le, i128, i128, to_le_bytes, [wire(le)]);
fixed_signed_count_cases!(fixed_isize_i64_be, isize, i64, to_be_bytes, [wire(as = i64, be)]);
fixed_signed_count_cases!(fixed_isize_i64_le, isize, i64, to_le_bytes, [wire(as = i64, le)]);
fixed_signed_count_cases!(fixed_isize_i128_be, isize, i128, to_be_bytes, [wire(as = i128, be)]);
fixed_signed_count_cases!(fixed_isize_i128_le, isize, i128, to_le_bytes, [wire(as = i128, le)]);

macro_rules! variable_signed_count_cases {
    ($module:ident, $logical:ty, $physical:ty, [$($attribute:meta),*],
        $sleb_minimum:expr, $uleb_minimum:expr, $vu_minimum:expr) => {
        mod $module {
            use super::*;

            signed_count_cases!(
                sleb, $logical, $physical, [wire(sleb128), $($attribute),*],
                &[(0, &[0x00]), (63, &[0x3f]), (64, &[0xc0, 0x00])],
                vec![
                    (vec![0x7f], "usize"),
                    ($sleb_minimum.to_vec(), if <$logical>::try_from(<$physical>::MIN).is_ok() {
                        "usize"
                    } else {
                        stringify!($logical)
                    }),
                ]
            );
            signed_count_cases!(
                zigzag_uleb, $logical, $physical, [wire(uleb128, zigzag), $($attribute),*],
                &[(0, &[0x00]), (63, &[0x7e]), (64, &[0x80, 0x01])],
                vec![
                    (vec![0x01], "usize"),
                    ($uleb_minimum.to_vec(), if <$logical>::try_from(<$physical>::MIN).is_ok() {
                        "usize"
                    } else {
                        stringify!($logical)
                    }),
                ]
            );
            signed_count_cases!(
                zigzag_vu, $logical, $physical, [wire(vu128, zigzag), $($attribute),*],
                &[(0, &[0x00]), (63, &[0x7e]), (64, &[0x80, 0x02])],
                vec![
                    (vec![0x01], "usize"),
                    ($vu_minimum.to_vec(), if <$logical>::try_from(<$physical>::MIN).is_ok() {
                        "usize"
                    } else {
                        stringify!($logical)
                    }),
                ]
            );
        }
    };
}

variable_signed_count_cases!(
    variable_i8,
    i8,
    i8,
    [],
    &[0x80, 0x7f],
    &[0xff, 0x01],
    &[0xbf, 0x03]
);
variable_signed_count_cases!(
    variable_i16,
    i16,
    i16,
    [],
    &[0x80, 0x80, 0x7e],
    &[0xff, 0xff, 0x03],
    &[0xdf, 0xff, 0x07]
);
variable_signed_count_cases!(
    variable_i32,
    i32,
    i32,
    [],
    &[0x80, 0x80, 0x80, 0x80, 0x78],
    &[0xff, 0xff, 0xff, 0xff, 0x0f],
    &[0xf3, 0xff, 0xff, 0xff, 0xff]
);
variable_signed_count_cases!(
    variable_i64,
    i64,
    i64,
    [],
    &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x7f],
    &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01],
    &[0xf7, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
);
variable_signed_count_cases!(
    variable_i128,
    i128,
    i128,
    [],
    &[
        0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80,
        0x80, 0x80, 0x80, 0x7e
    ],
    &[
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0x03
    ],
    &[0xff; 17]
);
variable_signed_count_cases!(
    variable_isize_i64, isize, i64, [wire(as = i64)],
    &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x7f],
    &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01],
    &[0xf7, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
);
variable_signed_count_cases!(
    variable_isize_i128, isize, i128, [wire(as = i128)],
    &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80,
      0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x7e],
    &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
      0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x03],
    &[0xff; 17]
);

#[test]
fn i8_progressive_count_boundary_emits_every_item_and_preserves_the_tail() -> TestResult {
    use fixed_i8::{Counted, CountedView, CountedWriteError};

    let mut expected = vec![0x91, 0x7f];
    for index in 0..127u16 {
        expected.extend_from_slice(&(0x3100 + index).to_be_bytes());
    }
    expected.push(0xa7);
    let mut output = vec![0xff; expected.len()];
    let written = Counted::builder(&mut output[..])
        .marker(0x91)?
        .items(|items| items.try_extend(0..127u16, |bar, index| bar.value(0x3100 + index)))?
        .tail(0xa7)?
        .finish()?;
    assert_eq!(written.as_bytes(), expected);
    let view = Counted::view(written.as_bytes())?;
    assert_eq!(view.tail(), 0xa7);
    assert_eq!(
        view.items()
            .iter()
            .last()
            .transpose()?
            .expect("last item")
            .view()
            .value(),
        0x317e
    );

    let mut output = [0xff; 259];
    let error = match Counted::builder(&mut output[..])
        .marker(0x91)?
        .items(|items| items.try_extend(0..128u16, |bar, index| bar.value(0x3100 + index)))
    {
        Ok(_) => panic!("128 items unexpectedly fit an i8 count"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::WriteError::Schema(CountedWriteError::Layout(wire_repr::LayoutError {
            field: "count"
        }))
    ));
    let mut expected = vec![0x91, 0x00];
    for index in 0..128u16 {
        expected.extend_from_slice(&(0x3100 + index).to_be_bytes());
    }
    expected.push(0xff);
    assert_eq!(output.as_slice(), expected);
    Ok(())
}

#[test]
fn i8_detached_count_boundary_emits_every_item_and_preserves_the_tail() -> TestResult {
    use fixed_i8::{
        CountedView, CountedWriteError, Envelope, EnvelopeBodyWriteError, EnvelopeVariant,
        EnvelopeView,
    };

    let mut expected = vec![0x6a, 0x04, 0x91, 0x7f];
    for index in 0..127u16 {
        expected.extend_from_slice(&(0x3100 + index).to_be_bytes());
    }
    expected.push(0xa7);
    let mut output = vec![0xff; expected.len()];
    let written = Envelope::builder(&mut output[..])
        .body(|body| {
            body.marker(0x91)
                .items(|items| items.extend(0..127u16, |bar, index| bar.value(0x3100 + index)))
                .tail(0xa7)
        })?
        .finish()?;
    assert_eq!(written.as_bytes(), expected);
    let view = Envelope::view(written.as_bytes())?;
    let EnvelopeVariant::Body(body) = view.variant();
    assert_eq!(body.tail(), 0xa7);
    assert_eq!(
        body.items()
            .iter()
            .last()
            .transpose()?
            .expect("last item")
            .view()
            .value(),
        0x317e
    );

    let mut output = [0xff; 261];
    let error = match Envelope::builder(&mut output[..]).body(|body| {
        body.marker(0x91)
            .items(|items| items.extend(0..128u16, |bar, index| bar.value(0x3100 + index)))
            .tail(0xa7)
    }) {
        Ok(_) => panic!("128 detached items unexpectedly fit an i8 count"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::WriteError::Schema(EnvelopeBodyWriteError::Body(CountedWriteError::Layout(
            wire_repr::LayoutError { field: "count" }
        )))
    ));
    let mut expected = vec![0x6a, 0x04, 0x91, 0x00];
    for index in 0..128u16 {
        expected.extend_from_slice(&(0x3100 + index).to_be_bytes());
    }
    expected.push(0xff);
    assert_eq!(output.as_slice(), expected);
    Ok(())
}

#[test]
fn isize_count_checks_the_logical_range_before_missing_array_geometry() {
    use fixed_isize_i128_le::{Counted, CountedViewError, Envelope, EnvelopeViewError};

    let count = isize::MAX as i128 + 1;
    let mut input = vec![0x91];
    input.extend_from_slice(&count.to_le_bytes());
    for error in [
        Counted::view(&input).err().expect("logical count overflow"),
        Counted::view_unchecked(&input)
            .err()
            .expect("unchecked logical count overflow"),
    ] {
        assert!(matches!(
            error,
            CountedViewError::CountValue(wire_repr::ScalarConversionError {
                offset: 1,
                from: "i128",
                to: "isize",
            })
        ));
    }
    let mut nested = vec![0x6a, 0x04];
    nested.extend_from_slice(&input);
    assert!(matches!(
        Envelope::view(nested),
        Err(EnvelopeViewError::VariantBody(
            CountedViewError::CountValue(wire_repr::ScalarConversionError {
                offset: 3,
                from: "i128",
                to: "isize",
            })
        ))
    ));
}

#[test]
fn detached_array_defers_into_iterator_and_mutable_item_callbacks_until_writing() -> TestResult {
    use core::cell::Cell;

    struct Source<'a> {
        started: &'a Cell<usize>,
    }

    impl IntoIterator for Source<'_> {
        type Item = (i32, i32);
        type IntoIter = std::array::IntoIter<Self::Item, 2>;

        fn into_iter(self) -> Self::IntoIter {
            self.started.set(self.started.get() + 1);
            [(17, -23), (41, -59)].into_iter()
        }
    }

    let started = Cell::new(0);
    let mut built = 0;
    let batch = <Batch as WireBuilder>::builder()
        .entries(|entries| {
            entries.extend(Source { started: &started }, |entry, (key, value)| {
                built += 1;
                entry.key(key).value(value)
            })
        })
        .enabled(true);
    assert_eq!(started.get(), 0);

    let mut output = [0u8; 23];
    Envelope::builder(&mut output[..])
        .batch(|_| batch)?
        .finish()?;
    assert_eq!(started.get(), 1);
    assert_eq!(built, 2);
    assert_eq!(output, BATCH_BYTES);
    Ok(())
}

#[test]
fn detached_enum_array_reports_fixed_output_shortages_at_every_boundary() {
    for available in 0..BATCH_BYTES.len() {
        let mut output = [0x55; 23];
        let error = match Envelope::builder(&mut output[..available]).batch(|batch| {
            batch
                .entries(|entries| {
                    entries.extend([(17, -23), (41, -59)], |entry, (key, value)| {
                        entry.key(key).value(value)
                    })
                })
                .enabled(true)
        }) {
            Ok(_) => panic!("short output with {available} bytes unexpectedly accepted"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            wire_repr::WriteError::Output(wire_repr::OutputError::NeedMore {
                request,
                available: reported,
            }) if reported == available && request.minimum_len > available
        ));
        if available == 22 {
            assert_eq!(&output[..22], &BATCH_BYTES[..22]);
            assert_eq!(output[22], 0x55);
        }
    }
}

#[test]
fn nested_detached_array_conversion_failure_keeps_field_sources_and_stops_streaming() {
    #[derive(WireView, WireBuilder)]
    struct CheckedItem {
        #[wire(as = u8)]
        value: usize,
    }

    let consumed = core::cell::Cell::new(0);
    let source = [1usize, 256, 2].into_iter().inspect(|_| {
        consumed.set(consumed.get() + 1);
    });
    let mut output = [0x55; 6];
    let error = match Foo::<Foo<CheckedItem>>::builder(&mut output[..]).items(|items| {
        items.item(|group| {
            group
                .items(|items| items.extend(source, |item, value| item.value(value)))
                .tail(7)
        })
    }) {
        Ok(_) => panic!("out-of-range detached array item unexpectedly wrote"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::WriteError::Schema(FooWriteError::Items(FooWriteError::Items(
            CheckedItemWriteError::ValueValue(_)
        )))
    ));
    assert_eq!(consumed.get(), 2);
    assert_eq!(output, [0, 0, 1, 0x55, 0x55, 0x55]);
}

#[test]
fn detached_enum_array_count_overflow_keeps_the_variant_and_controller_field_source() {
    let mut output = [0x55; 258];
    let error = match MethodChoice::builder(&mut output[..]).present(|method| {
        method.attributes(|attributes| attributes.extend(0..256, |attribute, _| attribute.data([])))
    }) {
        Ok(_) => panic!("unrepresentable detached array count unexpectedly wrote"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::WriteError::Schema(MethodChoicePresentWriteError::Body(
            MethodWriteError::Layout(wire_repr::LayoutError {
                field: "attributes_count"
            })
        ))
    ));
    let mut expected = [0u8; 258];
    expected[0] = 1;
    assert_eq!(output, expected);
}

#[test]
fn nested_detached_array_rejects_a_zero_width_item_without_wrapping_the_output_error() {
    #[derive(WireView, WireBuilder)]
    struct EmptyItem {
        bytes: [u8; 0],
    }

    let mut output = [0x55; 4];
    let error = match Foo::<Foo<EmptyItem>>::builder(&mut output[..]).items(|items| {
        items.item(|group| {
            group
                .items(|items| items.extend([[]], |item, bytes| item.bytes(bytes)))
                .tail(7)
        })
    }) {
        Ok(_) => panic!("zero-width detached array item unexpectedly wrote"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::WriteError::Output(wire_repr::OutputError::NonProgressItem { index: 0 })
    ));
    assert_eq!(output, [0, 0, 0x55, 0x55]);
}

#[test]
fn detached_array_manual_items_receive_isolated_byte_spans() -> TestResult {
    struct ChecksummedByte;

    impl WireBuilder for ChecksummedByte {
        type Builder = ();

        fn builder() -> Self::Builder {}
    }

    impl wire_repr::WireWrite<u8> for ChecksummedByte {
        type Error = core::convert::Infallible;

        fn write<O: wire_repr::Output>(
            value: u8,
            writer: &mut wire_repr::ChildWriter<'_, O>,
        ) -> Result<(), wire_repr::WriteError<Self::Error, O::GrowError>> {
            writer.write(&[value])?;
            let checksum = writer.as_bytes().iter().copied().fold(0, u8::wrapping_add);
            writer.write(&[checksum])?;
            Ok(())
        }
    }

    let mut output = [0u8; 8];
    Foo::<Foo<ChecksummedByte>>::builder(&mut output[..])
        .items(|groups| {
            groups.item(|group| {
                group
                    .items(|items| items.extend([3, 7], |(), value| value))
                    .tail(9)
            })
        })?
        .tail(10)?
        .finish()?;
    assert_eq!(output, [1, 2, 3, 3, 7, 7, 9, 10]);
    Ok(())
}

#[derive(WireView, WireBuilder)]
struct ExceptionHandler {
    #[wire(be)]
    start_pc: u16,
    #[wire(be)]
    end_pc: u16,
}

#[derive(WireView, WireBuilder)]
struct Code {
    #[wire(be)]
    code_length: u32,
    #[wire(bytes = code_length)]
    instructions: wire_repr::wire::Bytes,
    #[wire(be)]
    exception_count: u16,
    #[wire(counted_by = exception_count)]
    exceptions: wire_repr::wire::Array<ExceptionHandler>,
}

#[derive(WireView, WireBuilder)]
struct CodeWithTail {
    #[wire(be)]
    code_length: u32,
    #[wire(bytes = code_length)]
    instructions: wire_repr::wire::Bytes,
    #[wire(le)]
    exception_count: i32,
    marker: u8,
    #[wire(counted_by = exception_count)]
    exceptions: wire_repr::wire::Array<ExceptionHandler>,
    tail: u8,
}

#[test]
fn counted_array_after_dynamic_bytes_round_trips() -> TestResult {
    let input = [0, 0, 0, 2, 0xaa, 0xbb, 0, 1, 0, 3, 0, 5];
    let view = Code::view(input)?;
    assert_eq!(view.code_length(), 2);
    assert_eq!(view.instructions(), [0xaa, 0xbb]);
    assert_eq!(view.exception_count(), 1);
    let handler = view
        .exceptions()
        .iter()
        .next()
        .transpose()?
        .expect("handler");
    assert_eq!(handler.view().start_pc(), 3);
    assert_eq!(handler.view().end_pc(), 5);

    let selected = select(&view).include(|fields| fields.exception_count | fields.exceptions);
    assert_eq!(selected.bytes().collect::<Vec<_>>(), input[6..]);

    let mut output = [0u8; 12];
    Code::builder(&mut output[..])
        .instructions([0xaa, 0xbb])?
        .exceptions(|items| items.item(|item| item.start_pc(3).end_pc(5)))?
        .finish()?;
    assert_eq!(output, input);
    Ok(())
}

#[test]
fn dynamic_count_controller_before_intervening_fields_and_tail() -> TestResult {
    let input = [0, 0, 0, 1, 0xaa, 1, 0, 0, 0, 9, 0, 3, 0, 5, 7];
    let view = CodeWithTail::view(input)?;
    let count: i32 = view.exception_count();
    assert_eq!(count, 1);
    assert_eq!(view.marker(), 9);
    assert_eq!(view.tail(), 7);
    assert_eq!(
        view.exceptions()
            .iter()
            .next()
            .transpose()?
            .expect("handler")
            .view()
            .end_pc(),
        5
    );

    let mut output = [0u8; 15];
    CodeWithTail::builder(&mut output[..])
        .instructions([0xaa])?
        .marker(9)?
        .exceptions(|items| items.copy_from(view.exceptions()))?
        .tail(7)?
        .finish()?;
    assert_eq!(output, input);

    let mut nested = [0u8; 17];
    Foo::<CodeWithTail>::builder(&mut nested[..])
        .items(|items| {
            items.item(|code| {
                code.instructions([0xaa])
                    .marker(9)
                    .exceptions(|exceptions| {
                        exceptions.extend([(3, 5)], |handler, (start, end)| {
                            handler.start_pc(start).end_pc(end)
                        })
                    })
                    .tail(7)
            })
        })?
        .tail(8)?
        .finish()?;
    assert_eq!(
        nested,
        [1, 0, 0, 0, 1, 0xaa, 1, 0, 0, 0, 9, 0, 3, 0, 5, 7, 8]
    );

    let mut negative = [0u8; 9];
    negative[..5].copy_from_slice(&input[..5]);
    negative[5..].copy_from_slice(&(-1i32).to_le_bytes());
    assert!(matches!(
        CodeWithTail::view(negative),
        Err(CodeWithTailViewError::ExceptionCountValue(
            wire_repr::ScalarConversionError {
                offset: 5,
                from: "i32",
                to: "usize",
            }
        ))
    ));

    let error = match CodeWithTail::view(&input[..6]) {
        Ok(_) => panic!("truncated dynamic count unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        CodeWithTailViewError::ExceptionCountNeedMore(wire_repr::NeedMore {
            offset: 6,
            additional_at_least: 3,
        })
    ));

    let error = match CodeWithTail::view(&input[..13]) {
        Ok(_) => panic!("truncated array unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        CodeWithTailViewError::Exceptions(wire_repr::ArrayError::NeedMore(wire_repr::NeedMore {
            offset: 13,
            additional_at_least: 1,
        }))
    ));
    Ok(())
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
fn array_views_follow_into_iterator_and_fused_iterator_conventions() -> TestResult {
    let view = Foo::<Bar>::view([2, 0x11, 0x22, 0x33, 0x44, 9])?;
    let items = view.items();
    let mut borrowed = (&items).into_iter();
    fn assert_fused<I: core::iter::FusedIterator>(_iter: &I) {}
    assert_fused(&borrowed);
    assert_eq!(
        borrowed.next().transpose()?.expect("first").view().value(),
        0x1122
    );

    let values = view
        .items()
        .into_iter()
        .map(|item| item.map(|item| item.view().value()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(values, [0x1122, 0x3344]);
    Ok(())
}

#[test]
fn streaming_writer_patches_count_and_writes_each_item_progressively() -> TestResult {
    let mut output = [0u8; 6];
    let source = vec![0x1122, 0x3344];
    Foo::<Bar>::builder(&mut output[..])
        .items(|items| items.try_extend(source, |bar, value| bar.value(value)))?
        .tail(9)?
        .finish()?;
    assert_eq!(output, [2, 0x11, 0x22, 0x33, 0x44, 9]);
    Ok(())
}

#[test]
fn exact_array_forwarding_copies_one_validated_range_and_patches_count() -> TestResult {
    let source = Foo::<Bar>::view([2, 0x11, 0x22, 0x33, 0x44, 9])?;
    let mut output = [0u8; 6];
    Foo::<Bar>::builder(&mut output[..])
        .items(|items| items.copy_from(source.items()))?
        .tail(7)?
        .finish()?;
    assert_eq!(output, [2, 0x11, 0x22, 0x33, 0x44, 7]);
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

#[derive(WireView, WireBuilder)]
struct PlacedTerminal<T> {
    count: u8,
    #[wire(counted_by = count, at = 10)]
    items: wire_repr::wire::Array<T>,
}

#[derive(WireView, WireBuilder)]
struct Attribute {
    length: u8,
    #[wire(bytes = length)]
    data: wire_repr::wire::Bytes,
}

#[derive(WireView, WireBuilder)]
struct Method {
    attributes_count: u8,
    #[wire(counted_by = attributes_count)]
    attributes: wire_repr::wire::Array<Attribute>,
}

#[derive(WireView)]
struct Class {
    methods_count: u8,
    #[wire(counted_by = methods_count)]
    methods: wire_repr::wire::Array<Method>,
    tail: u8,
}

#[derive(WireView, WireBuilder)]
struct MethodWithCode {
    code_length: u8,
    #[wire(bytes = code_length)]
    code: wire_repr::wire::Bytes,
    attributes_count: u8,
    #[wire(counted_by = attributes_count)]
    attributes: wire_repr::wire::Array<Attribute>,
}

#[derive(WireView, WireBuilder)]
struct ClassWithCode {
    methods_count: u8,
    #[wire(counted_by = methods_count)]
    methods: wire_repr::wire::Array<MethodWithCode>,
    tail: u8,
}

#[derive(WireView)]
struct MethodWrapper {
    method: Method,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum MethodChoice {
    #[wire(value = 1)]
    Present(Method),
    #[wire(value = 2)]
    Empty,
}

#[derive(WireView)]
#[wire(selector = u8)]
enum OpenMethodChoice {
    #[wire(value = 1)]
    Present(Method),
    #[wire(unknown)]
    Unknown(wire_repr::wire::Bytes),
}

#[derive(WireView)]
struct UnboundedAttribute {
    #[wire(rest)]
    data: wire_repr::wire::Bytes,
}

#[derive(WireView)]
struct UnboundedMethod {
    attributes_count: u8,
    #[wire(counted_by = attributes_count)]
    attributes: wire_repr::wire::Array<UnboundedAttribute>,
}

#[derive(WireView)]
struct UnboundedClass {
    methods_count: u8,
    #[wire(counted_by = methods_count)]
    methods: wire_repr::wire::Array<UnboundedMethod>,
    tail: u8,
}

#[test]
fn nested_counted_arrays_stop_before_following_fields() -> TestResult {
    assert_eq!(
        <Method as wire_repr::WireView>::frame(&[0, 0x2a], 0)?
            .into_parts()
            .1,
        1,
    );
    let view = Class::view([1, 0, 0x2a])?;
    assert_eq!(view.tail(), 42);
    let mut methods = view.methods().iter();
    let method = methods.next().transpose()?.expect("one method");
    assert_eq!(method.as_bytes(), &[0]);
    assert_eq!(method.view().attributes_count(), 0);
    assert!(method.view().attributes().iter().next().is_none());
    assert!(methods.next().is_none());

    let view = Class::view([2, 2, 1, 0xaa, 2, 0xbb, 0xcc, 1, 0, 0x2a])?;
    assert_eq!(view.tail(), 42);
    let mut methods = view.methods().iter();
    let first = methods.next().transpose()?.expect("first method");
    assert_eq!(first.as_bytes(), &[2, 1, 0xaa, 2, 0xbb, 0xcc]);
    let first_view = first.view();
    let mut attributes = first_view.attributes().iter();
    assert_eq!(
        attributes
            .next()
            .transpose()?
            .expect("first attribute")
            .view()
            .data(),
        [0xaa]
    );
    assert_eq!(
        attributes
            .next()
            .transpose()?
            .expect("second attribute")
            .view()
            .data(),
        [0xbb, 0xcc],
    );
    assert!(attributes.next().is_none());
    let second = methods.next().transpose()?.expect("second method");
    assert_eq!(second.as_bytes(), &[1, 0]);
    assert_eq!(
        second
            .view()
            .attributes()
            .iter()
            .next()
            .transpose()?
            .expect("empty attribute")
            .view()
            .data(),
        [],
    );
    assert!(methods.next().is_none());
    Ok(())
}

#[test]
fn counted_prefix_after_dynamic_bytes_preserves_outer_tail() -> TestResult {
    let view = ClassWithCode::view([1, 2, 0xa1, 0xa2, 0, 42])?;
    assert_eq!(view.tail(), 42);
    let method = view.methods().iter().next().transpose()?.expect("method");
    assert_eq!(method.as_bytes(), [2, 0xa1, 0xa2, 0]);
    assert_eq!(method.view().code(), [0xa1, 0xa2]);

    let view = ClassWithCode::view([1, 2, 0xa1, 0xa2, 1, 0, 42])?;
    assert_eq!(view.tail(), 42);
    assert_eq!(
        view.methods()
            .iter()
            .next()
            .transpose()?
            .expect("method")
            .as_bytes(),
        [2, 0xa1, 0xa2, 1, 0],
    );
    Ok(())
}

#[test]
fn multiple_detached_array_levels_patch_counts_after_dynamic_data() -> TestResult {
    let expected = [1, 2, 2, 0xa1, 0xa2, 2, 1, 0xaa, 2, 0xbb, 0xcc, 0, 0, 42, 99];
    let mut output = [0u8; 15];
    let attributes = [&[0xaa][..], &[0xbb, 0xcc][..]];
    Foo::<ClassWithCode>::builder(&mut output[..])
        .items(|classes| {
            classes.item(|class| {
                class
                    .methods(|methods| {
                        methods.extend(
                            [(&[0xa1, 0xa2][..], &attributes[..]), (&[][..], &[][..])],
                            |method, (code, data)| {
                                method.code(code).attributes(|attributes| {
                                    attributes.extend(data, |attribute, data| attribute.data(data))
                                })
                            },
                        )
                    })
                    .tail(42)
            })
        })?
        .tail(99)?
        .finish()?;
    assert_eq!(output, expected);

    let outer = Foo::<ClassWithCode>::view(output)?;
    assert_eq!(outer.count(), 1);
    assert_eq!(outer.tail(), 99);
    let class = outer.items().iter().next().transpose()?.expect("class");
    let class = class.view();
    assert_eq!(class.methods_count(), 2);
    assert_eq!(class.tail(), 42);
    let mut methods = class.methods().iter();
    let first = methods.next().transpose()?.expect("first method");
    let first = first.view();
    assert_eq!(first.code(), [0xa1, 0xa2]);
    assert_eq!(first.attributes_count(), 2);
    let mut attributes = first.attributes().iter();
    assert_eq!(
        attributes
            .next()
            .transpose()?
            .expect("first attribute")
            .view()
            .data(),
        [0xaa]
    );
    assert_eq!(
        attributes
            .next()
            .transpose()?
            .expect("second attribute")
            .view()
            .data(),
        [0xbb, 0xcc]
    );
    assert!(attributes.next().is_none());
    let second = methods.next().transpose()?.expect("second method");
    assert_eq!(second.as_bytes(), [0, 0]);
    assert_eq!(second.view().attributes_count(), 0);
    assert!(second.view().attributes().is_empty());
    assert!(methods.next().is_none());
    Ok(())
}

#[test]
fn terminal_array_prefix_capability_propagates_through_structs_and_enums() -> TestResult {
    let mut methods = Method::views(&[0, 1, 0])?;
    assert_eq!(methods.next()?.expect("first method").as_bytes(), [0]);
    assert_eq!(methods.next()?.expect("second method").as_bytes(), [1, 0]);
    assert!(methods.next()?.is_none());

    let wrapper = MethodWrapper::view([0, 0x2a])?;
    assert_eq!(wrapper.method().attributes_count(), 0);
    let choice = MethodChoice::view([1, 0, 0x2a])?;
    assert!(matches!(
        choice.variant(),
        MethodChoiceVariant::Present(method) if method.attributes_count() == 0
    ));
    let open = OpenMethodChoice::view([1, 0, 0x2a])?;
    assert!(matches!(
        open.variant(),
        OpenMethodChoiceVariant::Present(method) if method.attributes_count() == 0
    ));

    let mut wrappers = MethodWrapper::views(&[0, 0])?;
    assert_eq!(
        wrappers.next()?.expect("first").method().attributes_count(),
        0
    );
    assert_eq!(
        wrappers
            .next()?
            .expect("second")
            .method()
            .attributes_count(),
        0
    );
    assert!(wrappers.next()?.is_none());

    let mut choices = MethodChoice::views(&[1, 0, 2])?;
    let first = choices.next()?.expect("first choice");
    match first.variant() {
        MethodChoiceVariant::Present(method) => assert_eq!(method.attributes_count(), 0),
        MethodChoiceVariant::Empty => panic!("expected method"),
    }
    assert!(matches!(
        choices.next()?.expect("second choice").variant(),
        MethodChoiceVariant::Empty
    ));
    assert!(choices.next()?.is_none());
    Ok(())
}

#[test]
fn nested_array_shortages_keep_absolute_offsets_without_eager_root_validation() -> TestResult {
    let error = match Class::view([1, 1, 2, 0xaa]) {
        Ok(_) => panic!("truncated nested attribute unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        ClassViewError::Methods(wire_repr::ArrayError::Item {
            index: 0,
            source: MethodViewError::Attributes(wire_repr::ArrayError::Item {
                index: 0,
                source: AttributeViewError::Data(wire_repr::NeedMore {
                    offset: 4,
                    additional_at_least: 1,
                }),
            }),
        })
    ));
    let error = match Class::view([1, 1, 0]) {
        Ok(_) => panic!("missing tail unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        ClassViewError::Tail(wire_repr::NeedMore {
            offset: 3,
            additional_at_least: 1,
        })
    ));
    let error = match Class::view([1, 0, 0x2a, 99]) {
        Ok(_) => panic!("trailing bytes unexpectedly accepted"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        ClassViewError::Trailing(wire_repr::TrailingBytes {
            offset: 3,
            trailing: 1,
        })
    ));

    let method = Method::view([1, 2, 0xaa])?;
    assert!(matches!(
        method.attributes().iter().next(),
        Some(Err(wire_repr::ArrayError::Item { index: 0, .. }))
    ));
    let method = Method::view([0, 0x2a])?;
    assert!(matches!(
        method.attributes().iter().next(),
        Some(Err(wire_repr::ArrayError::Trailing {
            offset: 1,
            trailing: 1
        }))
    ));
    Ok(())
}

#[test]
fn nonleading_items_never_claim_a_following_field() -> TestResult {
    assert_eq!(UnboundedClass::view([0, 0x2a])?.tail(), 42);
    let error = match UnboundedClass::view([1, 1, 7, 0x2a]) {
        Ok(_) => panic!("nonleading item unexpectedly framed before tail"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        UnboundedClassViewError::LayoutUnavailable { field: "methods" }
    ));
    Ok(())
}

#[test]
fn ordinary_generic_array_writer_keeps_placement_geometry() -> TestResult {
    let mut output = [0xff; 12];
    let written = PlacedTerminal::<Bar>::builder(&mut output[..])
        .items(|items| items.item(|bar| bar.value(0x1122)))?
        .finish()?;
    assert_eq!(
        written.as_bytes(),
        [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x11, 0x22]
    );
    Ok(())
}

#[test]
fn terminal_array_iterator_is_fused_after_item_failure() -> TestResult {
    let view = Terminal::<Variable>::view([2, 1, 7, 1])?;
    let mut iter = view.items().iter();
    let first = iter.next().transpose()?.expect("first item");
    assert_eq!(first.view().body(), &[7]);
    assert!(matches!(
        iter.next(),
        Some(Err(wire_repr::ArrayError::Item {
            index: 1,
            source: VariableViewError::Body(wire_repr::NeedMore {
                offset: 4,
                additional_at_least: 1,
            }),
        }))
    ));
    assert!(iter.next().is_none());
    Ok(())
}

#[test]
fn terminal_array_forwarding_validates_deferred_item_geometry_before_copying() -> TestResult {
    let source = Terminal::<Variable>::view([2, 1, 7, 1])?;
    let mut output = [0u8; 8];
    let error = match Foo::<Variable>::builder(&mut output[..])
        .items(|items| items.copy_from(source.items()))
    {
        Ok(_) => panic!("malformed terminal array unexpectedly copied"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::WriteError::Schema(FooWriteError::Items(wire_repr::ArrayError::Item {
            index: 1,
            ..
        }))
    ));
    Ok(())
}

#[test]
fn valid_terminal_array_forwards_exact_items_count_and_new_tail() -> TestResult {
    let source = Terminal::<Variable>::view([2, 1, 7, 2, 8, 9])?;
    let mut output = [0u8; 7];
    Foo::<Variable>::builder(&mut output[..])
        .items(|items| items.copy_from(source.items()))?
        .tail(5)?
        .finish()?;
    assert_eq!(output, [2, 1, 7, 2, 8, 9, 5]);
    Ok(())
}

#[test]
fn fixed_array_forwarding_revalidates_item_constants_before_copying() -> TestResult {
    let source = Terminal::<ConstantBar>::view([1, 2, 0])?;
    let mut output = [0u8; 4];
    let error = match Foo::<ConstantBar>::builder(&mut output[..])
        .items(|items| items.copy_from(source.items()))
    {
        Ok(_) => panic!("invalid fixed item unexpectedly copied"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        wire_repr::WriteError::Schema(FooWriteError::Items(wire_repr::ArrayError::Item {
            index: 0,
            ..
        }))
    ));
    Ok(())
}

#[test]
fn terminal_fixed_array_forwarding_preserves_exact_shortage_precedence() -> TestResult {
    let source = Terminal::<Bar>::view([2, 0x11, 0x22])?;
    let mut output = [0u8; 6];
    let error =
        match Foo::<Bar>::builder(&mut output[..]).items(|items| items.copy_from(source.items())) {
            Ok(_) => panic!("truncated fixed array unexpectedly copied"),
            Err(error) => error,
        };
    assert!(matches!(
        error,
        wire_repr::WriteError::Schema(FooWriteError::Items(wire_repr::ArrayError::NeedMore(
            wire_repr::NeedMore {
                additional_at_least: 2,
                ..
            }
        )))
    ));
    Ok(())
}

#[test]
fn terminal_array_placement_beyond_input_returns_shortage_before_getter() {
    let error = match PlacedTerminal::<Bar>::view([0]) {
        Ok(_) => panic!("out-of-input terminal array unexpectedly framed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        PlacedTerminalViewError::Items(wire_repr::ArrayError::NeedMore(wire_repr::NeedMore {
            offset: 1,
            additional_at_least: 9,
        }))
    ));
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
