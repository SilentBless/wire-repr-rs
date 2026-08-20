#![deny(missing_docs, unsafe_code)]

//! Public mutable fixed-absolute view and builder coverage.

use core::{
    convert::Infallible,
    sync::atomic::{AtomicUsize, Ordering},
};
use wire_repr::{EncodePlan, FixedCodec, wire_repr};

#[derive(Debug, PartialEq, Eq)]
struct Borrowed<'wire>(&'wire [u8]);

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

#[derive(Debug, PartialEq, Eq)]
enum PlanError {
    Rejected,
}

struct Failing;
impl FixedCodec for Failing {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type EncodeError = PlanError;
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
            Err(PlanError::Rejected)
        } else {
            Ok([value])
        }
    }
}

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

static ZERO_PLANS: AtomicUsize = AtomicUsize::new(0);
struct Zero;
impl FixedCodec for Zero {
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
        ZERO_PLANS.fetch_add(1, Ordering::Relaxed);
        Ok([])
    }
}

struct HugePlan;
impl EncodePlan for HugePlan {
    fn encoded_len(&self) -> usize {
        usize::MAX
    }
    fn write_into(&self, _: &mut [u8]) {}
}

static HUGE_PLANS: AtomicUsize = AtomicUsize::new(0);
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
        HUGE_PLANS.fetch_add(1, Ordering::Relaxed);
        Ok(HugePlan)
    }
}

static WIDE_PLANS: AtomicUsize = AtomicUsize::new(0);
struct Wide;
impl FixedCodec for Wide {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 3]
    where
        Self: 'value;
    const WIDTH: usize = 3;
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        bytes[0]
    }
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        WIDE_PLANS.fetch_add(1, Ordering::Relaxed);
        Ok([value; 3])
    }
}

wire_repr! {
    /// A sparse layout declared opposite to physical order.
    pub absolute layout Packet {
        /// The trailing word.
        tail @ 4: BeU16;
        /// Borrowed bytes in the middle.
        borrowed @ 1: crate::Borrowing;
        /// The leading byte.
        head @ 0: U8;
    }
    /// Planning failures in declaration order.
    pub absolute layout Problem {
        /// First declaration, physically later.
        first @ 2: crate::Failing;
        /// Second declaration, physically first.
        wrong @ 0: crate::WrongPlan;
    }
    /// A zero-width structural failure.
    pub absolute layout ZeroLayout {
        /// Invalid codec.
        zero @ 3: crate::Zero;
    }
    /// An overflowing structural failure.
    pub absolute layout OverflowLayout {
        /// Invalid codec extent.
        huge @ 1: crate::Huge;
    }
    /// An overlapping structural failure.
    pub absolute layout OverlapLayout {
        /// Earlier wide field.
        wide @ 0: crate::Wide;
        /// Later overlap.
        later @ 2: U8;
    }
}

#[test]
fn mutable_parsing_preserves_suffix_and_immutable_conversions() {
    let mut input = [7, 0xca, 0xfe, 0xaa, 0x12, 0x34, 0x99];
    let (mut view, suffix) = PacketViewMut::parse_prefix_mut(&mut input).expect("valid prefix");
    assert_eq!(suffix, [0x99]);
    assert_eq!(view.head(), 7);
    assert_eq!(view.borrowed(), Borrowed(&[0xca, 0xfe]));
    assert_eq!(view.tail(), 0x1234);
    assert_eq!(view.as_bytes(), &[7, 0xca, 0xfe, 0xaa, 0x12, 0x34]);
    view.set_head(8).expect("built-in plan");
    assert_eq!(view.as_view().head(), 8);
    assert_eq!(
        view.into_view().as_bytes(),
        &[8, 0xca, 0xfe, 0xaa, 0x12, 0x34]
    );

    let mut exact = [7, 0xca, 0xfe, 0xaa, 0x12, 0x34];
    assert_eq!(
        PacketViewMut::parse_exact_mut(&mut exact)
            .expect("exact")
            .tail(),
        0x1234
    );
    assert!(matches!(
        PacketViewMut::parse_exact_mut(&mut [7, 0, 0, 0, 0, 1, 2]),
        Err(PacketError::TrailingBytes {
            expected: 6,
            actual: 7
        })
    ));
}

#[test]
fn setters_are_atomic_and_write_only_the_field_span() {
    let mut bytes = [0x10, 0x20, 0x30];
    let mut view = ProblemViewMut::parse_exact_mut(&mut bytes).expect("valid bytes");
    assert!(matches!(
        view.set_first(0),
        Err(ProblemMutationError::FieldFirst(PlanError::Rejected))
    ));
    assert_eq!(view.as_bytes(), &[0x10, 0x20, 0x30]);
    assert!(matches!(
        view.set_wrong(9),
        Err(ProblemMutationError::InvalidPlanLength {
            field: "wrong",
            expected: 2,
            actual: 1
        })
    ));
    assert_eq!(view.as_bytes(), &[0x10, 0x20, 0x30]);
    view.set_first(0x77).expect("exact plan");
    assert_eq!(view.as_bytes(), &[0x10, 0x20, 0x77]);
}

#[test]
fn builder_is_atomic_preserves_gaps_and_returns_a_mutable_view() {
    let borrowed = [0xca, 0xfe];
    let mut output = [0xde, 0xaa, 0xbb, 0xcc, 0xad, 0xbe, 0x99];
    let (mut view, suffix) = PacketBuilder::new()
        .tail(0x1234)
        .borrowed(Borrowed(&borrowed))
        .head(7)
        .build_into(&mut output)
        .expect("complete builder");
    assert_eq!(view.as_bytes(), &[7, 0xca, 0xfe, 0xcc, 0x12, 0x34]);
    assert_eq!(suffix, [0x99]);
    view.set_head(8).expect("remains mutable");
    assert_eq!(output, [8, 0xca, 0xfe, 0xcc, 0x12, 0x34, 0x99]);

    let mut unchanged = [0x55; 5];
    assert!(matches!(
        ProblemBuilder::new().wrong(2).build_into(&mut unchanged),
        Err(ProblemWriteError::MissingField { field: "first" })
    ));
    assert_eq!(unchanged, [0x55; 5]);
    assert!(matches!(
        ProblemBuilder::new()
            .first(0)
            .wrong(2)
            .build_into(&mut unchanged),
        Err(ProblemWriteError::FieldFirst(PlanError::Rejected))
    ));
    assert_eq!(unchanged, [0x55; 5]);
    assert!(matches!(
        ProblemBuilder::new()
            .first(1)
            .wrong(2)
            .build_into(&mut unchanged),
        Err(ProblemWriteError::InvalidPlanLength {
            field: "wrong",
            expected: 2,
            actual: 1
        })
    ));
    assert_eq!(unchanged, [0x55; 5]);
    assert!(matches!(
        PacketBuilder::new()
            .tail(1)
            .borrowed(Borrowed(&borrowed))
            .head(2)
            .build_into(&mut unchanged),
        Err(PacketWriteError::OutputTooShort {
            needed: 6,
            available: 5
        })
    ));
    assert_eq!(unchanged, [0x55; 5]);
}

#[test]
fn structural_errors_precede_missing_values_and_planning() {
    ZERO_PLANS.store(0, Ordering::Relaxed);
    let mut output = [0xa5];
    assert!(matches!(
        ZeroLayoutBuilder::new().build_into(&mut output),
        Err(ZeroLayoutWriteError::InvalidCodecWidth { offset: 3 })
    ));
    assert_eq!(ZERO_PLANS.load(Ordering::Relaxed), 0);
    assert_eq!(output, [0xa5]);

    HUGE_PLANS.store(0, Ordering::Relaxed);
    assert_eq!(OverflowLayout::WIDTH, usize::MAX);
    assert!(matches!(
        OverflowLayoutViewMut::parse_prefix_mut(&mut []),
        Err(OverflowLayoutError::InvalidCodecExtent { offset: 1, width }) if width == usize::MAX
    ));
    assert!(matches!(
        OverflowLayoutBuilder::new().huge(0).build_into(&mut output),
        Err(OverflowLayoutWriteError::InvalidCodecExtent { offset: 1, width }) if width == usize::MAX
    ));
    assert_eq!(HUGE_PLANS.load(Ordering::Relaxed), 0);
    assert_eq!(output, [0xa5]);

    WIDE_PLANS.store(0, Ordering::Relaxed);
    assert!(matches!(
        OverlapLayoutBuilder::new().build_into(&mut output),
        Err(OverlapLayoutWriteError::OverlappingFields {
            earlier_offset: 0,
            later_offset: 2
        })
    ));
    assert_eq!(WIDE_PLANS.load(Ordering::Relaxed), 0);
    assert_eq!(output, [0xa5]);
}
