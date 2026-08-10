use core::convert::Infallible;
use core::num::NonZeroUsize;

use wire_repr::codec::{self, EncodePlan};
use wire_repr::{
    BeI16, BeI32, BeI64, BeI128, BeU16, BeU24, BeU32, BeU64, BeU128, FixedCodec, I8, LeI16, LeI32,
    LeI64, LeI128, LeU16, LeU24, LeU32, LeU64, LeU128, PrefixCodec, PrefixExtent, U8,
    U24RangeError,
};

fn completed_plan<'value, C>(value: C::Value<'value>) -> C::Plan<'value>
where
    C: FixedCodec<EncodeError = Infallible> + 'value,
{
    match C::plan(value) {
        Ok(plan) => plan,
        Err(error) => match error {},
    }
}

fn render_plan<const N: usize>(plan: impl EncodePlan) -> [u8; N] {
    assert_eq!(plan.encoded_len(), N);
    let mut output = [0xa5; N];
    plan.write_into(&mut output);
    output
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
fn plans_are_caller_buffer_driven() {
    let plan = completed_plan::<BeU32>(0x1234_5678);
    assert_eq!(plan.encoded_len(), 4);
    let mut output = [0xa5; 6];
    plan.write_into(&mut output[1..5]);
    assert_eq!(output, [0xa5, 0x12, 0x34, 0x56, 0x78, 0xa5]);
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

struct BorrowedValue<'wire>(&'wire [u8]);
struct BorrowedPlan<'value>(&'value [u8]);
impl EncodePlan for BorrowedPlan<'_> {
    fn encoded_len(&self) -> usize {
        self.0.len()
    }
    fn write_into(&self, output: &mut [u8]) {
        output.copy_from_slice(self.0);
    }
}
struct Borrowing;
impl FixedCodec for Borrowing {
    type Value<'wire>
        = BorrowedValue<'wire>
    where
        Self: 'wire;
    type EncodeError = Infallible;
    type Plan<'value>
        = BorrowedPlan<'value>
    where
        Self: 'value;
    const WIDTH: usize = 2;
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        BorrowedValue(bytes)
    }
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok(BorrowedPlan(value.0))
    }
}
#[test]
fn gat_values_and_plans_can_borrow() {
    let input = [0xca, 0xfe];
    let value = Borrowing::decode(&input);
    assert_eq!(Borrowing::plan(value).map(render_plan::<2>), Ok(input));
}

struct Marker;
impl FixedCodec for Marker {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;
    const WIDTH: usize = 1;
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        bytes[0]
    }
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([value])
    }
}
#[test]
fn fixed_decode_accepts_raw_values_for_consumer_classification() {
    let raw = Marker::decode(&[0xff]);
    assert_eq!(raw, 0xff);
    assert!(matches!(raw, 0xff));
    assert_eq!(Marker::decode(&[1]), 1);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TinyDecodeError {
    Empty,
    Incomplete,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TinyEncodeError {
    ReservedMarker,
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
            [0, _, ..] => Ok(PrefixExtent::new(NonZeroUsize::MIN.saturating_add(1))),
            [_, ..] => Ok(PrefixExtent::new(NonZeroUsize::MIN)),
        }
    }
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        if bytes[0] == 0 {
            bytes[1]
        } else {
            bytes[0] - 1
        }
    }
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        match value.checked_add(1) {
            Some(encoded) => Ok([encoded]),
            None => Err(TinyEncodeError::ReservedMarker),
        }
    }
}

#[test]
fn prefix_validation_distinguishes_empty_and_incomplete_input() {
    assert_eq!(
        TinyPrefix::validate_prefix(&[]),
        Err(TinyDecodeError::Empty)
    );
    assert_eq!(
        TinyPrefix::validate_prefix(&[0]),
        Err(TinyDecodeError::Incomplete)
    );
}

#[test]
fn prefix_extent_preserves_exact_spans_and_decode_follows_validation() {
    let canonical_input = [42, 0x99];
    let canonical_extent = TinyPrefix::validate_prefix(&canonical_input).unwrap();
    let (canonical_encoded, canonical_suffix) =
        canonical_extent.split_input(&canonical_input).unwrap();
    assert_eq!(canonical_encoded, &[42]);
    assert_eq!(canonical_suffix, &[0x99]);
    assert_eq!(TinyPrefix::decode(canonical_encoded), 41);

    let noncanonical_input = [0, 41, 0x99];
    let noncanonical_extent = TinyPrefix::validate_prefix(&noncanonical_input).unwrap();
    let (noncanonical_encoded, noncanonical_suffix) = noncanonical_extent
        .split_input(&noncanonical_input)
        .unwrap();
    assert_eq!(noncanonical_encoded, &[0, 41]);
    assert_eq!(noncanonical_suffix, &[0x99]);
    assert_eq!(TinyPrefix::decode(noncanonical_encoded), 41);
    assert_eq!(
        noncanonical_extent.encoded_len(),
        NonZeroUsize::new(2).unwrap()
    );

    let short_input = [0xa5, 0x5a];
    let overclaimed = PrefixExtent::new(NonZeroUsize::new(3).unwrap());
    assert_eq!(overclaimed.split_input(&short_input), None);

    assert_eq!(TinyPrefix::plan(41), Ok([42]));
    assert_eq!(TinyPrefix::plan(255), Err(TinyEncodeError::ReservedMarker));
}

#[test]
fn prefix_decode_panics_when_validation_preconditions_are_violated() {
    assert!(std::panic::catch_unwind(|| TinyPrefix::decode(&[])).is_err());
    assert!(std::panic::catch_unwind(|| TinyPrefix::decode(&[0])).is_err());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminatedDecodeError {
    Incomplete,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminatedEncodeError {
    EmbeddedTerminator,
    LengthOverflow,
}
struct TerminatedPlan<'value> {
    value: &'value [u8],
    encoded_len: usize,
}
impl EncodePlan for TerminatedPlan<'_> {
    fn encoded_len(&self) -> usize {
        self.encoded_len
    }
    fn write_into(&self, output: &mut [u8]) {
        let (value, terminator) = output.split_at_mut(self.value.len());
        value.copy_from_slice(self.value);
        terminator.copy_from_slice(&[0]);
    }
}
struct Terminated;
impl PrefixCodec for Terminated {
    type Value<'wire>
        = &'wire [u8]
    where
        Self: 'wire;
    type DecodeError = TerminatedDecodeError;
    type EncodeError = TerminatedEncodeError;
    type Plan<'value>
        = TerminatedPlan<'value>
    where
        Self: 'value;
    fn validate_prefix(bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        match bytes.iter().position(|byte| *byte == 0) {
            Some(value_len) => match value_len.checked_add(1).and_then(NonZeroUsize::new) {
                Some(encoded_len) => Ok(PrefixExtent::new(encoded_len)),
                None => Err(TerminatedDecodeError::Incomplete),
            },
            None => Err(TerminatedDecodeError::Incomplete),
        }
    }
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        &bytes[..bytes.len() - 1]
    }
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        if value.contains(&0) {
            Err(TerminatedEncodeError::EmbeddedTerminator)
        } else {
            match value.len().checked_add(1) {
                Some(encoded_len) => Ok(TerminatedPlan { value, encoded_len }),
                None => Err(TerminatedEncodeError::LengthOverflow),
            }
        }
    }
}

#[test]
fn borrowed_prefix_values_plans_and_exact_spans_are_preserved() {
    let input = [b'a', b'b', 0, 0x99];
    let extent = Terminated::validate_prefix(&input).unwrap();
    let (encoded, suffix) = extent.split_input(&input).unwrap();
    let value = Terminated::decode(encoded);
    assert_eq!(encoded, &[b'a', b'b', 0]);
    assert_eq!(suffix, &[0x99]);
    assert_eq!(value, b"ab");
    assert_eq!(value.as_ptr(), input.as_ptr());
    assert_eq!(
        Terminated::plan(value).map(render_plan::<3>),
        Ok([b'a', b'b', 0])
    );
    assert_eq!(
        Terminated::plan(&[b'a', 0]).map(|_| ()),
        Err(TerminatedEncodeError::EmbeddedTerminator)
    );
    assert_eq!(
        Terminated::validate_prefix(b"ab"),
        Err(TerminatedDecodeError::Incomplete)
    );
}

#[test]
fn root_and_codec_facades_expose_prefix_codec_contracts() {
    let root_extent = <TinyPrefix as PrefixCodec>::validate_prefix(&[42]).unwrap();
    let codec_extent: codec::PrefixExtent =
        <TinyPrefix as codec::PrefixCodec>::validate_prefix(&[42]).unwrap();
    assert_eq!(root_extent.encoded_len(), NonZeroUsize::MIN);
    assert_eq!(codec_extent.encoded_len(), NonZeroUsize::MIN);
}
