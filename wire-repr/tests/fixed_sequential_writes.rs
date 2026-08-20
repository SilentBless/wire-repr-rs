#![deny(missing_docs, unsafe_code)]

//! Public mutable fixed-sequential view and builder coverage.

use core::{
    convert::Infallible,
    sync::atomic::{AtomicUsize, Ordering},
};

use wire_repr::{EncodePlan, FixedCodec, wire_repr};

/// A borrowed two-byte builder value.
#[derive(Debug, PartialEq, Eq)]
struct Borrowed<'wire>(&'wire [u8]);

/// A two-byte codec with borrowed values.
struct Borrowing;

impl FixedCodec for Borrowing {
    type Value<'wire>
        = Borrowed<'wire>
    where
        Self: 'wire;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 2]
    where
        Self: 'value;
    const WIDTH: usize = 2;
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        Borrowed(bytes)
    }
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([value.0[0], value.0[1]])
    }
}

/// An encoding error used by the fallible codec.
#[derive(Debug, PartialEq, Eq)]
enum EncodeError {
    Rejected,
}

/// A one-byte codec that rejects zero while planning.
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

/// Counts attempted plans for the structurally invalid zero-width codec.
static ZERO_WIDTH_PLAN_CALLS: AtomicUsize = AtomicUsize::new(0);

/// A zero-width codec that violates the fixed codec law.
struct ZeroWidth;

impl FixedCodec for ZeroWidth {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 0]
    where
        Self: 'value;
    const WIDTH: usize = 0;
    fn decode<'wire>(_: &'wire [u8]) -> Self::Value<'wire> {
        0
    }
    fn plan<'value>(_: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        ZERO_WIDTH_PLAN_CALLS.fetch_add(1, Ordering::Relaxed);
        Ok([])
    }
}

/// A plan reporting an enormous encoded length without allocating bytes.
struct HugePlan;

impl EncodePlan for HugePlan {
    fn encoded_len(&self) -> usize {
        usize::MAX
    }
    fn write_into(&self, _: &mut [u8]) {}
}

/// Counts attempted plans for the codec in the overflowing layout.
static HUGE_PLAN_CALLS: AtomicUsize = AtomicUsize::new(0);

/// A codec whose valid width reaches the largest representable extent.
struct Huge;

impl FixedCodec for Huge {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type EncodeError = Infallible;
    type Plan<'value>
        = HugePlan
    where
        Self: 'value;
    const WIDTH: usize = usize::MAX;
    fn decode<'wire>(_: &'wire [u8]) -> Self::Value<'wire> {
        0
    }
    fn plan<'value>(_: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        HUGE_PLAN_CALLS.fetch_add(1, Ordering::Relaxed);
        Ok(HugePlan)
    }
}

/// A deliberately invalid successful encoding plan.
struct WrongPlan;

impl FixedCodec for WrongPlan {
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
    /// A mixed physical-order layout with opaque spacing.
    pub layout Mixed {
        /// The trailing big-endian word.
        field tail: BeU16 { position: 4; }
        align { position: 3; boundary: 4; }
        padding { position: 2; length: 2; }
        /// The leading byte.
        field head: U8 { position: 1; }
    }

    /// A layout combining scalar and borrowed builder values.
    pub layout BorrowedPacket {
        /// Borrowed bytes physically after the scalar.
        field borrowed: codec(Borrowing) { position: 2; }
        /// A scalar first byte.
        field scalar: U8 { position: 1; }
    }

    /// A layout whose codecs exercise atomic preparation failures.
    pub layout Problem {
        /// The first declared field.
        field failing: codec(Failing) { position: 1; }
        /// The second declared field.
        field wrong: codec(WrongPlan) { position: 2; }
    }

    /// A layout that rejects a zero-width custom fixed codec while building.
    pub layout ZeroWidthPacket {
        /// The invalid custom field.
        field zero: codec(ZeroWidth) { position: 1; }
    }

    /// A layout whose padding advances beyond the largest representable extent.
    pub layout OverflowPacket {
        /// The maximal-width custom field.
        field huge: codec(Huge) { position: 1; }
        padding { position: 2; length: 1; }
    }

    /// A layout proving builder preparation uses declaration rather than physical order.
    pub layout DeclarationOrder {
        /// The first declared field, physically second.
        field first: codec(Failing) { position: 2; }
        /// The second declared field, physically first.
        field second: codec(Failing) { position: 1; }
    }
}

#[test]
fn mutable_views_validate_split_and_convert_without_exposing_mutable_bytes() {
    let mut input = [7, 0xaa, 0xbb, 0xcc, 0x12, 0x34, 0x99];
    let (mut view, suffix) = MixedViewMut::parse_prefix_mut(&mut input).expect("valid prefix");
    assert_eq!(suffix, [0x99]);
    assert_eq!(view.head(), 7);
    assert_eq!(view.tail(), 0x1234);
    assert_eq!(
        view.as_view().as_bytes(),
        &[7, 0xaa, 0xbb, 0xcc, 0x12, 0x34]
    );
    view.set_head(8).expect("built-in plan succeeds");
    let immutable = view.into_view();
    assert_eq!(immutable.head(), 8);
    assert_eq!(immutable.as_bytes(), &[8, 0xaa, 0xbb, 0xcc, 0x12, 0x34]);
    let mut exact_bytes = [7, 0xaa, 0xbb, 0xcc, 0x12, 0x34];
    let exact = MixedViewMut::parse_exact_mut(&mut exact_bytes).expect("exact mutable layout");
    assert_eq!(exact.tail(), 0x1234);
    assert!(matches!(
        MixedViewMut::parse_exact_mut(&mut [7, 0, 0, 0, 0, 1, 2]),
        Err(MixedError::TrailingBytes { .. })
    ));
}

#[test]
fn setters_preflight_before_changing_the_owned_field() {
    let mut bytes = [1, 2, 3];
    let mut view = ProblemViewMut::parse_exact_mut(&mut bytes).expect("valid bytes");
    assert!(matches!(
        view.set_failing(0),
        Err(ProblemMutationError::FieldFailing(EncodeError::Rejected))
    ));
    assert_eq!(view.as_bytes(), &[1, 2, 3]);
    assert!(matches!(
        view.set_wrong(9),
        Err(ProblemMutationError::InvalidPlanLength {
            field: "wrong",
            expected: 2,
            actual: 1
        })
    ));
    assert_eq!(view.as_bytes(), &[1, 2, 3]);
}

#[test]
fn builders_are_atomic_and_write_in_physical_order() {
    let borrowed = [0xca, 0xfe];
    let mut borrowed_output = [0; 3];
    let (borrowed_view, borrowed_suffix) = BorrowedPacketBuilder::new()
        .scalar(7)
        .borrowed(Borrowed(&borrowed))
        .build_into(&mut borrowed_output)
        .expect("scalar and borrowed values infer one lifetime");
    assert!(borrowed_suffix.is_empty());
    assert_eq!(borrowed_view.as_bytes(), &[7, 0xca, 0xfe]);
    assert_eq!(borrowed_view.borrowed(), Borrowed(&borrowed));

    let mut unchanged = [0x55; 6];
    assert!(matches!(
        ProblemBuilder::new().wrong(2).build_into(&mut unchanged),
        Err(ProblemWriteError::MissingField { field: "failing" })
    ));
    assert_eq!(unchanged, [0x55; 6]);
    assert!(matches!(
        ProblemBuilder::new()
            .failing(0)
            .wrong(2)
            .build_into(&mut unchanged),
        Err(ProblemWriteError::FieldFailing(EncodeError::Rejected))
    ));
    assert_eq!(unchanged, [0x55; 6]);
    assert!(matches!(
        ProblemBuilder::new()
            .failing(1)
            .wrong(2)
            .build_into(&mut unchanged),
        Err(ProblemWriteError::InvalidPlanLength {
            field: "wrong",
            expected: 2,
            actual: 1
        })
    ));
    assert_eq!(unchanged, [0x55; 6]);

    let mut short = [0x44; 5];
    assert!(matches!(
        MixedBuilder::new()
            .tail(0x1234)
            .head(7)
            .build_into(&mut short),
        Err(MixedWriteError::OutputTooShort {
            needed: 6,
            available: 5
        })
    ));
    assert_eq!(short, [0x44; 5]);

    let mut output = [0xde, 0xaa, 0xbb, 0xcc, 0xad, 0xbe, 0x99];
    let (mut view, suffix) = MixedBuilder::new()
        .tail(0x1234)
        .head(7)
        .build_into(&mut output)
        .expect("complete builder");
    assert_eq!(view.as_bytes(), &[7, 0xaa, 0xbb, 0xcc, 0x12, 0x34]);
    view.set_head(8).expect("built view remains mutable");
    assert_eq!(view.as_bytes(), &[8, 0xaa, 0xbb, 0xcc, 0x12, 0x34]);
    assert_eq!(suffix, [0x99]);
    assert_eq!(output, [8, 0xaa, 0xbb, 0xcc, 0x12, 0x34, 0x99]);
}

#[test]
fn builders_reject_zero_width_codecs_before_missing_values_or_writing() {
    ZERO_WIDTH_PLAN_CALLS.store(0, Ordering::Relaxed);
    let mut output = [0xa5];
    assert!(matches!(
        ZeroWidthPacketBuilder::new().build_into(&mut output),
        Err(ZeroWidthPacketWriteError::InvalidCodecWidth { field: "zero" })
    ));
    assert_eq!(ZERO_WIDTH_PLAN_CALLS.load(Ordering::Relaxed), 0);
    assert_eq!(output, [0xa5]);
}

#[test]
fn overflowing_fixed_extents_fail_before_slicing_or_writing() {
    assert_eq!(OverflowPacket::WIDTH, usize::MAX);
    assert!(matches!(
        OverflowPacket::view(&[]).with_remainder(),
        Err(OverflowPacketError::InvalidLayoutExtent {
            position: 2,
            offset: usize::MAX,
            advance: 1
        })
    ));
    let mut input = [];
    assert!(matches!(
        OverflowPacketViewMut::parse_prefix_mut(&mut input),
        Err(OverflowPacketError::InvalidLayoutExtent {
            position: 2,
            offset: usize::MAX,
            advance: 1
        })
    ));
    HUGE_PLAN_CALLS.store(0, Ordering::Relaxed);
    let mut output = [0x5a];
    assert!(matches!(
        OverflowPacketBuilder::new().huge(0).build_into(&mut output),
        Err(OverflowPacketWriteError::InvalidLayoutExtent {
            position: 2,
            offset: usize::MAX,
            advance: 1
        })
    ));
    assert_eq!(HUGE_PLAN_CALLS.load(Ordering::Relaxed), 0);
    assert_eq!(output, [0x5a]);
}

#[test]
fn builder_plan_errors_follow_declaration_order() {
    let mut output = [0x3c; 2];
    assert!(matches!(
        DeclarationOrderBuilder::new()
            .first(0)
            .second(0)
            .build_into(&mut output),
        Err(DeclarationOrderWriteError::FieldFirst(
            EncodeError::Rejected
        ))
    ));
    assert_eq!(output, [0x3c; 2]);
}
