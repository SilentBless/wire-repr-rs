use core::{convert::Infallible, num::NonZeroUsize};

use wire_repr::{ExactWidthError, FixedCodec, PrefixCodec, PrefixExtent, wire_repr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PrefixError {
    Empty,
}

struct Length;
impl PrefixCodec for Length {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type DecodeError = PrefixError;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;
    fn validate_prefix(bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        if bytes.is_empty() {
            Err(PrefixError::Empty)
        } else {
            Ok(PrefixExtent::new(NonZeroUsize::MIN))
        }
    }
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        bytes[0]
    }
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([value])
    }
}

struct Borrowing;
impl FixedCodec for Borrowing {
    type Value<'wire>
        = &'wire [u8]
    where
        Self: 'wire;
    type EncodeError = ExactWidthError;
    type Plan<'value>
        = [u8; 2]
    where
        Self: 'value;
    const WIDTH: usize = 2;
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        bytes
    }
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        match value {
            [a, b] => Ok([*a, *b]),
            _ => Err(ExactWidthError::new(2, value.len())),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EncodeError {
    Rejected,
}
struct Failing;
impl FixedCodec for Failing {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type EncodeError = EncodeError;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;
    const WIDTH: usize = 1;
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        bytes[0]
    }
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        if value == 0 {
            Err(EncodeError::Rejected)
        } else {
            Ok([value])
        }
    }
}
struct Wrong;
impl FixedCodec for Wrong {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;
    const WIDTH: usize = 2;
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        bytes[0]
    }
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([value])
    }
}

wire_repr! {
    pub layout DynamicWrite {
        field length: prefix(Length) { position: 1; }
        field body: region(length) { position: 2; }
        padding { position: 3; length: 1; }
        align { position: 4; boundary: 4; }
        field tail: BeU16 {
            position: 5;
            projections {
                bit tail_low: 0;
            }
        }
        field borrowed: codec(Borrowing) { position: 6; }
    }
    pub layout AtomicProblems {
        field prefix: prefix(Length) { position: 1; }
        field failing: codec(Failing) { position: 2; }
        field wrong: codec(Wrong) { position: 3; }
    }
    pub layout NoEligibleSetters {
        field length: prefix(Length) { position: 1; }
        field body: region(length) { position: 2; }
    }
}

#[test]
fn mutable_dynamic_view_preserves_boundaries_and_updates_only_eligible_spans() {
    let mut input = [2, b'a', b'b', 0xee, 0x12, 0x34, 0xca, 0xfe, 0x99];
    let (mut view, suffix) = DynamicWriteViewMut::parse_prefix_mut(&mut input).unwrap();
    assert_eq!(suffix, [0x99]);
    assert_eq!(
        view.as_bytes(),
        &[2, b'a', b'b', 0xee, 0x12, 0x34, 0xca, 0xfe]
    );
    assert_eq!(view.length_encoded(), &[2]);
    assert_eq!(view.length(), 2);
    assert_eq!(view.body(), b"ab");
    assert_eq!(view.tail(), 0x1234);
    assert!(!view.tail_low());
    assert_eq!(view.borrowed(), &[0xca, 0xfe]);
    assert_eq!(view.as_view().body(), b"ab");
    view.set_tail(0xabcd).unwrap();
    assert!(view.tail_low());
    let replacement = [1, 2];
    view.set_borrowed(&replacement).unwrap();
    assert_eq!(view.as_bytes(), &[2, b'a', b'b', 0xee, 0xab, 0xcd, 1, 2]);
    let immutable = view.into_view();
    assert_eq!(immutable.length_encoded(), &[2]);
    assert_eq!(immutable.body(), b"ab");
    assert_eq!(immutable.tail(), 0xabcd);
    assert_eq!(immutable.borrowed(), &[1, 2]);
}

#[test]
fn mutable_dynamic_layout_without_eligible_setters_compiles_and_parses() {
    fn assert_error_type(_: Option<NoEligibleSettersMutationError>) {}

    assert_error_type(None);
    let mut bytes = [2, b'a', b'b'];
    let view = NoEligibleSettersViewMut::parse_exact_mut(&mut bytes).unwrap();
    assert_eq!(view.length(), 2);
    assert_eq!(view.body(), b"ab");
}

#[test]
fn mutable_dynamic_setters_are_atomic_and_parse_failures_do_not_write() {
    let mut bytes = [0, 1, 2, 3];
    let mut view = AtomicProblemsViewMut::parse_exact_mut(&mut bytes).unwrap();
    assert!(matches!(
        view.set_failing(0),
        Err(AtomicProblemsMutationError::FieldFailing(
            EncodeError::Rejected
        ))
    ));
    assert_eq!(view.as_bytes(), &[0, 1, 2, 3]);
    assert!(matches!(
        view.set_wrong(9),
        Err(AtomicProblemsMutationError::InvalidPlanLength {
            field: "wrong",
            expected: 2,
            actual: 1
        })
    ));
    assert_eq!(view.as_bytes(), &[0, 1, 2, 3]);

    let mut short = [2, b'a'];
    let before = short;
    assert!(matches!(
        DynamicWriteViewMut::parse_prefix_mut(&mut short),
        Err(DynamicWriteError::InputTooShort {
            position: 2,
            expected: 2,
            available: 1
        })
    ));
    assert_eq!(short, before);
}
