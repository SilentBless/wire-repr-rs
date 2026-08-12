use core::convert::Infallible;
use core::num::NonZeroUsize;
use core::sync::atomic::{AtomicUsize, Ordering};

use wire_repr::{EncodePlan, FixedCodec, PrefixCodec, PrefixExtent, wire_repr};

static MIXED_VALIDATIONS: AtomicUsize = AtomicUsize::new(0);
static MIXED_DECODE_LENGTH: AtomicUsize = AtomicUsize::new(0);
static ORDER_VALIDATIONS: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TinyDecodeError {
    Empty,
    Incomplete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TinyEncodeError {
    Reserved,
}

struct TinyPrefix;

impl PrefixCodec for TinyPrefix {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type DecodeError = TinyDecodeError;
    type EncodeError = TinyEncodeError;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;

    fn validate_prefix(bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        match bytes {
            [] => Err(TinyDecodeError::Empty),
            [0] => Err(TinyDecodeError::Incomplete),
            [0, _, ..] => Ok(PrefixExtent::new(NonZeroUsize::new(2).unwrap())),
            [_, ..] => Ok(PrefixExtent::new(NonZeroUsize::MIN)),
        }
    }

    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        match bytes {
            [0, value] => *value,
            [value] => value - 1,
            _ => panic!("decode must receive one exact validated prefix"),
        }
    }

    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        value
            .checked_add(1)
            .map(|encoded| [encoded])
            .ok_or(TinyEncodeError::Reserved)
    }
}

struct MixedPrefix;

impl PrefixCodec for MixedPrefix {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type DecodeError = TinyDecodeError;
    type EncodeError = TinyEncodeError;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;

    fn validate_prefix(bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        MIXED_VALIDATIONS.fetch_add(1, Ordering::Relaxed);
        match bytes {
            [] => Err(TinyDecodeError::Empty),
            [0] => Err(TinyDecodeError::Incomplete),
            [0, _, ..] => Ok(PrefixExtent::new(NonZeroUsize::new(2).unwrap())),
            [_, ..] => Ok(PrefixExtent::new(NonZeroUsize::MIN)),
        }
    }

    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        MIXED_DECODE_LENGTH.store(bytes.len(), Ordering::Relaxed);
        match bytes {
            [0, value] => *value,
            [value] => value - 1,
            _ => panic!("decode must receive one exact validated prefix"),
        }
    }

    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        value
            .checked_add(1)
            .map(|encoded| [encoded])
            .ok_or(TinyEncodeError::Reserved)
    }
}

struct MustNotValidatePrefix;

impl PrefixCodec for MustNotValidatePrefix {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type DecodeError = TinyDecodeError;
    type EncodeError = TinyEncodeError;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;

    fn validate_prefix(_: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        panic!("later prefix validation must not run after an invalid fixed width")
    }

    fn decode<'wire>(_: &'wire [u8]) -> Self::Value<'wire> {
        panic!("an unvalidated prefix must not be decoded")
    }

    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([value])
    }
}

struct PhysicalOrderPrefix;

impl PrefixCodec for PhysicalOrderPrefix {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type DecodeError = TinyDecodeError;
    type EncodeError = TinyEncodeError;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;

    fn validate_prefix(bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        ORDER_VALIDATIONS.fetch_add(1, Ordering::Relaxed);
        match bytes {
            [] => Err(TinyDecodeError::Empty),
            [0] => Err(TinyDecodeError::Incomplete),
            [0, _, ..] => Ok(PrefixExtent::new(NonZeroUsize::new(2).unwrap())),
            [_, ..] => Ok(PrefixExtent::new(NonZeroUsize::MIN)),
        }
    }

    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        match bytes {
            [0, value] => *value,
            [value] => value - 1,
            _ => panic!("decode must receive one exact validated prefix"),
        }
    }

    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        value
            .checked_add(1)
            .map(|encoded| [encoded])
            .ok_or(TinyEncodeError::Reserved)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminatedError {
    Incomplete,
}

struct Terminated;

impl PrefixCodec for Terminated {
    type Value<'wire>
        = &'wire [u8]
    where
        Self: 'wire;
    type DecodeError = TerminatedError;
    type EncodeError = Infallible;
    type Plan<'value>
        = TerminatedPlan<'value>
    where
        Self: 'value;

    fn validate_prefix(bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        let Some(index) = bytes.iter().position(|byte| *byte == 0) else {
            return Err(TerminatedError::Incomplete);
        };
        let encoded_len = index.checked_add(1).and_then(NonZeroUsize::new);
        match encoded_len {
            Some(encoded_len) => Ok(PrefixExtent::new(encoded_len)),
            None => Err(TerminatedError::Incomplete),
        }
    }

    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        &bytes[..bytes.len() - 1]
    }

    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok(TerminatedPlan { value })
    }
}

struct TerminatedPlan<'value> {
    value: &'value [u8],
}

impl EncodePlan for TerminatedPlan<'_> {
    fn encoded_len(&self) -> usize {
        self.value.len() + 1
    }

    fn write_into(&self, output: &mut [u8]) {
        let (value, terminator) = output.split_at_mut(self.value.len());
        value.copy_from_slice(self.value);
        terminator[0] = 0;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Rejected;

struct RejectPrefix;

impl PrefixCodec for RejectPrefix {
    type Value<'wire>
        = ()
    where
        Self: 'wire;
    type DecodeError = Rejected;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;

    fn validate_prefix(_bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        Err(Rejected)
    }

    fn decode<'wire>(_bytes: &'wire [u8]) -> Self::Value<'wire> {}

    fn plan<'value>(_value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([0])
    }
}

struct Overclaim;

impl PrefixCodec for Overclaim {
    type Value<'wire>
        = ()
    where
        Self: 'wire;
    type DecodeError = Infallible;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;

    fn validate_prefix(_bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        Ok(PrefixExtent::new(NonZeroUsize::new(4).unwrap()))
    }

    fn decode<'wire>(_bytes: &'wire [u8]) -> Self::Value<'wire> {}

    fn plan<'value>(_value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([0])
    }
}

struct BorrowingFixed;

impl FixedCodec for BorrowingFixed {
    type Value<'wire>
        = &'wire [u8]
    where
        Self: 'wire;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 2]
    where
        Self: 'value;

    const WIDTH: usize = 2;

    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        bytes
    }

    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([value[0], value[1]])
    }
}

struct ZeroWidth;

impl FixedCodec for ZeroWidth {
    type Value<'wire>
        = ()
    where
        Self: 'wire;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 0]
    where
        Self: 'value;

    const WIDTH: usize = 0;

    fn decode<'wire>(_: &'wire [u8]) -> Self::Value<'wire> {}

    fn plan<'value>(_value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([])
    }
}

wire_repr! {
    pub layout Mixed {
        field tail: BeU16 { position: 3; }
        field value: prefix(MixedPrefix) { position: 2; }
        field tag: U8 { position: 1; }
    }

    pub layout Borrowed {
        field tail: U8 { position: 2; }
        field body: prefix(Terminated) { position: 1; }
    }

    pub layout Multiple {
        field second: prefix(TinyPrefix) { position: 3; }
        field middle: U8 { position: 2; }
        field first: prefix(TinyPrefix) { position: 1; }
    }

    pub layout Adjacent {
        field second: prefix(TinyPrefix) { position: 2; }
        field first: prefix(TinyPrefix) { position: 1; }
    }

    pub layout Failed {
        field value: prefix(RejectPrefix) { position: 1; }
    }

    pub layout Overclaimed {
        field value: prefix(Overclaim) { position: 1; }
    }

    pub layout ShortAfterPrefix {
        field value: prefix(TinyPrefix) { position: 1; }
        field tail: BeU16 { position: 2; }
    }

    pub layout PhysicalOrderDynamic {
        field value: prefix(PhysicalOrderPrefix) { position: 2; }
        field head: BeU16 { position: 1; }
    }

    pub layout BorrowedFixedAfterPrefix {
        field value: prefix(TinyPrefix) { position: 1; }
        field bytes: codec(BorrowingFixed) { position: 2; }
    }

    pub layout InvalidFixedWidth {
        field invalid: codec(ZeroWidth) { position: 1; }
        field value: prefix(MustNotValidatePrefix) { position: 2; }
    }
}

#[test]
fn mixed_layout_preserves_exact_prefix_encoding_suffix_and_declaration_api() {
    MIXED_VALIDATIONS.store(0, Ordering::Relaxed);
    MIXED_DECODE_LENGTH.store(0, Ordering::Relaxed);
    let input = [7, 0, 41, 0x12, 0x34, 0xaa];
    let (view, suffix) = MixedView::parse_prefix(&input).expect("mixed prefix layout should parse");

    assert_eq!(view.as_bytes(), &[7, 0, 41, 0x12, 0x34]);
    assert_eq!(suffix, &[0xaa]);
    assert_eq!(view.tag(), 7);
    assert_eq!(view.value_raw(), &[0, 41]);
    assert_eq!(MIXED_VALIDATIONS.load(Ordering::Relaxed), 1);
    assert_eq!(view.value(), 41);
    assert_eq!(MIXED_DECODE_LENGTH.load(Ordering::Relaxed), 2);
    assert_eq!(MIXED_VALIDATIONS.load(Ordering::Relaxed), 1);
    assert_eq!(view.tail(), 0x1234);

    let canonical =
        MixedView::parse_exact(&[7, 42, 0x12, 0x34]).expect("canonical prefix layout should parse");
    assert_eq!(canonical.value_raw(), &[42]);
    assert_eq!(canonical.value(), 41);
}

#[test]
fn exact_parse_reports_dynamic_represented_length() {
    assert!(matches!(
        ShortAfterPrefixView::parse_exact(&[42, 0x12, 0x34, 0xaa]),
        Err(ShortAfterPrefixError::TrailingBytes {
            expected: 3,
            actual: 4
        })
    ));
}

#[test]
fn borrowing_and_multiple_prefix_boundaries_are_exact() {
    let borrowed_input = [b'a', b'b', 0, 9];
    let borrowed =
        BorrowedView::parse_exact(&borrowed_input).expect("borrowing prefix layout should parse");
    assert_eq!(borrowed.body_raw(), &[b'a', b'b', 0]);
    assert_eq!(borrowed.body(), b"ab");
    assert_eq!(borrowed.body().as_ptr(), borrowed_input.as_ptr());
    assert_eq!(borrowed.tail(), 9);

    let multiple =
        MultipleView::parse_exact(&[42, 7, 0, 8]).expect("multiple prefix layout should parse");
    assert_eq!(multiple.first_raw(), &[42]);
    assert_eq!(multiple.first(), 41);
    assert_eq!(multiple.middle(), 7);
    assert_eq!(multiple.second_raw(), &[0, 8]);
    assert_eq!(multiple.second(), 8);

    let adjacent =
        AdjacentView::parse_exact(&[42, 0, 8]).expect("adjacent prefix fields should parse");
    assert_eq!(adjacent.first_raw(), &[42]);
    assert_eq!(adjacent.first(), 41);
    assert_eq!(adjacent.second_raw(), &[0, 8]);
    assert_eq!(adjacent.second(), 8);
}

#[test]
fn codec_and_structural_failures_are_mapped_without_blind_slicing() {
    assert!(matches!(
        FailedView::parse_prefix(&[1]),
        Err(FailedError::FieldValue(Rejected))
    ));
    assert!(matches!(
        OverclaimedView::parse_prefix(&[1, 2]),
        Err(OverclaimedError::InvalidPrefixExtent {
            position: 1,
            claimed: 4,
            available: 2
        })
    ));
    assert!(matches!(
        ShortAfterPrefixView::parse_prefix(&[42, 0x12]),
        Err(ShortAfterPrefixError::InputTooShort {
            position: 2,
            expected: 2,
            available: 1
        })
    ));
}

#[test]
fn borrowed_fixed_getters_use_the_exact_post_prefix_span() {
    let input = [42, 0xca, 0xfe];
    let view = BorrowedFixedAfterPrefixView::parse_exact(&input)
        .expect("borrowed fixed field after prefix should parse");
    assert_eq!(view.bytes(), &[0xca, 0xfe]);
    assert_eq!(view.bytes().as_ptr(), input[1..].as_ptr());
}

#[test]
fn dynamic_validation_uses_physical_not_declaration_order() {
    ORDER_VALIDATIONS.store(0, Ordering::Relaxed);
    assert!(matches!(
        PhysicalOrderDynamicView::parse_prefix(&[0x12]),
        Err(PhysicalOrderDynamicError::InputTooShort {
            position: 1,
            expected: 2,
            available: 1
        })
    ));
    assert_eq!(ORDER_VALIDATIONS.load(Ordering::Relaxed), 0);
}

#[test]
fn zero_width_fixed_fields_fail_before_prefix_validation() {
    assert!(matches!(
        InvalidFixedWidthView::parse_prefix(&[42]),
        Err(InvalidFixedWidthError::InvalidCodecWidth { position: 1 })
    ));
}
