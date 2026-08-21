#![deny(missing_docs, unsafe_code)]

//! Transformed dynamic byte-range source coverage.

use core::cell::Cell;

use wire_repr::{BeU16, FixedCodec, OutputTooShortError, RangeSource, U8, wire_repr};

#[derive(Debug, Eq, PartialEq)]
enum RangeError {
    InvalidWire,
    Unaligned,
    TooLarge,
}

std::thread_local! {
    static WORD_TO_CALLS: Cell<usize> = const { Cell::new(0) };
    static WORD_FROM_CALLS: Cell<usize> = const { Cell::new(0) };
    static ENDPOINT_TO_CALLS: Cell<usize> = const { Cell::new(0) };
    static ENDPOINT_FROM_CALLS: Cell<usize> = const { Cell::new(0) };
    static SHARED_FROM_CALLS: Cell<usize> = const { Cell::new(0) };
}

struct Words;

impl RangeSource<U8> for Words {
    type Error = RangeError;

    fn to_bytes(value: <U8 as FixedCodec>::Value<'_>) -> Result<usize, Self::Error> {
        WORD_TO_CALLS.with(|calls| calls.set(calls.get() + 1));
        if value == u8::MAX {
            return Err(RangeError::InvalidWire);
        }
        Ok(usize::from(value) * 4)
    }

    fn from_bytes(bytes: usize) -> Result<<U8 as FixedCodec>::Value<'static>, Self::Error> {
        WORD_FROM_CALLS.with(|calls| calls.set(calls.get() + 1));
        if !bytes.is_multiple_of(4) {
            return Err(RangeError::Unaligned);
        }
        u8::try_from(bytes / 4).map_err(|_| RangeError::TooLarge)
    }
}

struct TotalEndpoint;

impl RangeSource<BeU16> for TotalEndpoint {
    type Error = RangeError;

    fn to_bytes(value: <BeU16 as FixedCodec>::Value<'_>) -> Result<usize, Self::Error> {
        ENDPOINT_TO_CALLS.with(|calls| calls.set(calls.get() + 1));
        if value == u16::MAX {
            return Err(RangeError::InvalidWire);
        }
        Ok(usize::from(value))
    }

    fn from_bytes(bytes: usize) -> Result<<BeU16 as FixedCodec>::Value<'static>, Self::Error> {
        ENDPOINT_FROM_CALLS.with(|calls| calls.set(calls.get() + 1));
        if bytes == 3 {
            return Err(RangeError::InvalidWire);
        }
        u16::try_from(bytes).map_err(|_| RangeError::TooLarge)
    }
}

struct SharedWords;

impl RangeSource<U8> for SharedWords {
    type Error = RangeError;

    fn to_bytes(value: <U8 as FixedCodec>::Value<'_>) -> Result<usize, Self::Error> {
        Ok(usize::from(value) * 4)
    }

    fn from_bytes(bytes: usize) -> Result<<U8 as FixedCodec>::Value<'static>, Self::Error> {
        SHARED_FROM_CALLS.with(|calls| calls.set(calls.get() + 1));
        if !bytes.is_multiple_of(4) {
            return Err(RangeError::Unaligned);
        }
        u8::try_from(bytes / 4).map_err(|_| RangeError::TooLarge)
    }
}

wire_repr! {
    pub layout WordFrame {
        length @ 1: U8 { range_source: crate::Words; };
        payload @ 2: bytes(length);
        tail @ 3: U8;
    }

    pub layout Datagram {
        endpoint @ 1: BeU16 { range_source: crate::TotalEndpoint; };
        payload @ 2: bytes_to(endpoint);
        tail @ 3: U8;
    }

    pub layout SharedWordFrame {
        length @ 1: U8 { range_source: crate::SharedWords; };
        first @ 2: bytes(length);
        second @ 3: bytes(length);
    }
}

#[test]
fn relative_word_sources_frame_raw_values_and_prepare_once() {
    WORD_TO_CALLS.with(|calls| calls.set(0));
    WORD_FROM_CALLS.with(|calls| calls.set(0));
    let input = [2, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0xaa];
    let (view, suffix) = WordFrame::view(&input).with_remainder().unwrap();
    assert_eq!(view.length(), 2);
    assert_eq!(view.payload(), &[1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(view.tail(), 9);
    assert_eq!(view.as_bytes(), &input[..10]);
    assert_eq!(suffix, &[0xaa]);
    assert_eq!(WORD_TO_CALLS.with(|calls| calls.get()), 1);

    assert!(matches!(
        WordFrame::view(&[u8::MAX]).with_remainder(),
        Err(WordFrameError::RangeSourceFieldLength {
            position: 2,
            source_position: 1,
            error: RangeError::InvalidWire,
        })
    ));

    let plan = WordFrameBuilder::new()
        .payload(&[0x10, 0x20, 0x30, 0x40])
        .tail(0x99)
        .prepare()
        .unwrap();
    assert_eq!(WORD_FROM_CALLS.with(|calls| calls.get()), 1);
    let initial = [0xa5; 6];
    let mut output = initial;
    let (built, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(built.as_bytes(), &[1, 0x10, 0x20, 0x30, 0x40, 0x99]);
    assert!(suffix.is_empty());
    assert_eq!(WORD_FROM_CALLS.with(|calls| calls.get()), 1);

    let initial = [0x5a; 5];
    let mut unchanged = initial;
    assert!(matches!(
        WordFrameBuilder::new()
            .payload(&[0; 3])
            .tail(1)
            .build_into(&mut unchanged),
        Err(WordFrameWriteError::RangeSourceFieldLength {
            position: 2,
            source_position: 1,
            value: 3,
            error: RangeError::Unaligned,
        })
    ));
    assert_eq!(unchanged, initial);
}

#[test]
fn absolute_total_endpoints_preserve_ordering_and_source_errors() {
    ENDPOINT_TO_CALLS.with(|calls| calls.set(0));
    ENDPOINT_FROM_CALLS.with(|calls| calls.set(0));
    let input = [0, 5, 0x10, 0x20, 0x30, 0x99, 0xaa];
    let (view, suffix) = Datagram::view(&input).with_remainder().unwrap();
    assert_eq!(view.endpoint(), 5);
    assert_eq!(view.payload(), &[0x10, 0x20, 0x30]);
    assert_eq!(view.tail(), 0x99);
    assert_eq!(view.as_bytes(), &input[..6]);
    assert_eq!(suffix, &[0xaa]);
    assert!(matches!(
        Datagram::view(&[0, 1]).with_remainder(),
        Err(DatagramError::RangeEndBeforeStart {
            position: 2,
            source_position: 1,
            end: 1,
            start: 2,
        })
    ));
    assert!(matches!(
        Datagram::view(&[0xff, 0xff]).with_remainder(),
        Err(DatagramError::RangeSourceFieldEndpoint {
            position: 2,
            source_position: 1,
            error: RangeError::InvalidWire,
        })
    ));

    let initial = [0x5a; 3];
    let mut unchanged = initial;
    assert!(matches!(
        DatagramBuilder::new()
            .payload(&[0x11])
            .tail(0x44)
            .build_into(&mut unchanged),
        Err(DatagramWriteError::RangeSourceFieldEndpoint {
            position: 2,
            source_position: 1,
            value: 3,
            error: RangeError::InvalidWire,
        })
    ));
    assert_eq!(unchanged, initial);

    let plan = DatagramBuilder::new()
        .payload(&[0x11, 0x22, 0x33])
        .tail(0x44)
        .prepare()
        .unwrap();
    assert_eq!(ENDPOINT_FROM_CALLS.with(|calls| calls.get()), 2);
    let mut output = [0; 6];
    let (built, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(built.as_bytes(), &[0, 5, 0x11, 0x22, 0x33, 0x44]);
    assert!(suffix.is_empty());
    assert_eq!(ENDPOINT_FROM_CALLS.with(|calls| calls.get()), 2);
}

#[test]
fn shared_sources_compare_byte_geometry_before_one_conversion() {
    SHARED_FROM_CALLS.with(|calls| calls.set(0));
    let mut output = [0; 9];
    let (view, _) = SharedWordFrameBuilder::new()
        .first(&[1, 2, 3, 4])
        .second(&[5, 6, 7, 8])
        .build_into(&mut output)
        .unwrap();
    assert_eq!(view.as_bytes(), &[1, 1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(SHARED_FROM_CALLS.with(|calls| calls.get()), 1);

    SHARED_FROM_CALLS.with(|calls| calls.set(0));
    let initial = [0xa5; 8];
    let mut output = initial;
    assert!(matches!(
        SharedWordFrameBuilder::new()
            .first(&[1, 2, 3, 4])
            .second(&[5, 6, 7])
            .build_into(&mut output),
        Err(SharedWordFrameWriteError::ConflictingRangeSources {
            source_position: 1,
            first_range_position: 2,
            conflicting_range_position: 3,
            expected: 4,
            actual: 3,
        })
    ));
    assert_eq!(SHARED_FROM_CALLS.with(|calls| calls.get()), 0);
    assert_eq!(output, initial);
}

#[test]
fn short_prepared_commit_is_capacity_only_and_atomic() {
    WORD_FROM_CALLS.with(|calls| calls.set(0));
    let plan = WordFrameBuilder::new()
        .payload(&[0, 1, 2, 3])
        .tail(4)
        .prepare()
        .unwrap();
    assert_eq!(WORD_FROM_CALLS.with(|calls| calls.get()), 1);
    let initial = [0xa5; 5];
    let mut output = initial;
    assert!(matches!(
        plan.commit_into(&mut output),
        Err(OutputTooShortError {
            required: 6,
            available: 5,
        })
    ));
    assert_eq!(WORD_FROM_CALLS.with(|calls| calls.get()), 1);
    assert_eq!(output, initial);
}
