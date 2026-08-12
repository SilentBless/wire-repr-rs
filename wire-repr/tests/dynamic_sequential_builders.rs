use core::convert::Infallible;
use core::num::NonZeroUsize;
use core::sync::atomic::{AtomicUsize, Ordering};

use wire_repr::{EncodePlan, FixedCodec, PrefixCodec, PrefixExtent, wire_repr};

struct TwoBytePrefix;

impl PrefixCodec for TwoBytePrefix {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type DecodeError = Infallible;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 2]
    where
        Self: 'value;

    fn validate_prefix(_: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        Ok(PrefixExtent::new(NonZeroUsize::new(2).unwrap()))
    }

    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        bytes[1]
    }

    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([0xf0, value])
    }
}

static MISSING_PLANS: AtomicUsize = AtomicUsize::new(0);

struct MissingCount;

impl FixedCodec for MissingCount {
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
        MISSING_PLANS.fetch_add(1, Ordering::Relaxed);
        Ok([value])
    }
}

static ZERO_WIDTH_PLANS: AtomicUsize = AtomicUsize::new(0);

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
        ZERO_WIDTH_PLANS.fetch_add(1, Ordering::Relaxed);
        Ok([])
    }
}

#[derive(Debug, Eq, PartialEq)]
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
    fn plan<'value>(_: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Err(PlanError::Rejected)
    }
}

struct WrongFixedPlan;

impl FixedCodec for WrongFixedPlan {
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

struct EmptyPrefixPlan;

impl PrefixCodec for EmptyPrefixPlan {
    type Value<'wire>
        = ()
    where
        Self: 'wire;
    type DecodeError = Infallible;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 0]
    where
        Self: 'value;

    fn validate_prefix(_: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        Ok(PrefixExtent::new(NonZeroUsize::MIN))
    }
    fn decode<'wire>(_: &'wire [u8]) -> Self::Value<'wire> {}
    fn plan<'value>(_: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([])
    }
}

struct MaxPrefixPlan;

impl EncodePlan for MaxPrefixPlan {
    fn encoded_len(&self) -> usize {
        usize::MAX
    }
    fn write_into(&self, _: &mut [u8]) {}
}

struct OverflowingPrefixPlan;

impl PrefixCodec for OverflowingPrefixPlan {
    type Value<'wire>
        = ()
    where
        Self: 'wire;
    type DecodeError = Infallible;
    type EncodeError = Infallible;
    type Plan<'value>
        = MaxPrefixPlan
    where
        Self: 'value;

    fn validate_prefix(_: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        Ok(PrefixExtent::new(NonZeroUsize::MIN))
    }
    fn decode<'wire>(_: &'wire [u8]) -> Self::Value<'wire> {}
    fn plan<'value>(_: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok(MaxPrefixPlan)
    }
}

static CAPACITY_FIRST_PLANS: AtomicUsize = AtomicUsize::new(0);
static CAPACITY_SECOND_PLANS: AtomicUsize = AtomicUsize::new(0);
static CAPACITY_WRITES: AtomicUsize = AtomicUsize::new(0);

struct CapacityFirst;
struct CapacitySecond;

macro_rules! capacity_codec {
    ($name:ident, $plans:ident) => {
        impl FixedCodec for $name {
            type Value<'wire>
                = u8
            where
                Self: 'wire;
            type EncodeError = Infallible;
            type Plan<'value>
                = CapacityPlan
            where
                Self: 'value;
            const WIDTH: usize = 1;
            fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
                bytes[0]
            }
            fn plan<'value>(
                value: Self::Value<'value>,
            ) -> Result<Self::Plan<'value>, Self::EncodeError> {
                $plans.fetch_add(1, Ordering::Relaxed);
                Ok(CapacityPlan(value))
            }
        }
    };
}

struct CapacityPlan(u8);
impl EncodePlan for CapacityPlan {
    fn encoded_len(&self) -> usize {
        1
    }
    fn write_into(&self, output: &mut [u8]) {
        CAPACITY_WRITES.fetch_add(1, Ordering::Relaxed);
        output[0] = self.0;
    }
}
capacity_codec!(CapacityFirst, CAPACITY_FIRST_PLANS);
capacity_codec!(CapacitySecond, CAPACITY_SECOND_PLANS);

static ORDER_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
static FIRST_PLAN_ORDER: AtomicUsize = AtomicUsize::new(usize::MAX);
static SECOND_PLAN_ORDER: AtomicUsize = AtomicUsize::new(usize::MAX);
static FIRST_WRITE_ORDER: AtomicUsize = AtomicUsize::new(usize::MAX);
static SECOND_WRITE_ORDER: AtomicUsize = AtomicUsize::new(usize::MAX);

struct First;
struct Second;
struct OrderedPlan {
    byte: u8,
    write_order: &'static AtomicUsize,
}

impl EncodePlan for OrderedPlan {
    fn encoded_len(&self) -> usize {
        1
    }
    fn write_into(&self, output: &mut [u8]) {
        self.write_order.store(
            ORDER_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            Ordering::Relaxed,
        );
        output[0] = self.byte;
    }
}

macro_rules! ordered_codec {
    ($name:ident, $byte:literal, $plan_order:ident, $write_order:ident) => {
        impl FixedCodec for $name {
            type Value<'wire>
                = ()
            where
                Self: 'wire;
            type EncodeError = Infallible;
            type Plan<'value>
                = OrderedPlan
            where
                Self: 'value;
            const WIDTH: usize = 1;
            fn decode<'wire>(_: &'wire [u8]) -> Self::Value<'wire> {}
            fn plan<'value>(
                _: Self::Value<'value>,
            ) -> Result<Self::Plan<'value>, Self::EncodeError> {
                $plan_order.store(
                    ORDER_SEQUENCE.fetch_add(1, Ordering::Relaxed),
                    Ordering::Relaxed,
                );
                Ok(OrderedPlan {
                    byte: $byte,
                    write_order: &$write_order,
                })
            }
        }
    };
}
ordered_codec!(First, 1, FIRST_PLAN_ORDER, FIRST_WRITE_ORDER);
ordered_codec!(Second, 2, SECOND_PLAN_ORDER, SECOND_WRITE_ORDER);

wire_repr! {
    /// A mixed dynamic layout for builder coverage.
    pub layout Stem {
        /// A trailing fixed word.
        field tail: BeU16 { position: 7; }
        /// The auto-derived range length.
        field length: U8 { position: 1; }
        /// An ordinary fixed prefix field.
        field tag: U8 { position: 2; }
        /// A planned variable-width prefix.
        field prefix: prefix(TwoBytePrefix) { position: 3; }
        /// Opaque range bytes.
        field body: bytes(current_pos..current_pos + length) { position: 4; }
        padding { position: 5; length: 1; }
        align { position: 6; boundary: 4; }
    }

    /// A layout with a fixed length source.
    pub layout EmptyRange {
        /// The auto-derived fixed length.
        field length: U8 { position: 1; }
        /// The possibly empty range.
        field body: bytes(current_pos..current_pos + length) { position: 2; }
    }

    /// Two ranges sharing one source.
    pub layout SharedRanges {
        /// The auto-derived shared length.
        field length: U8 { position: 1; }
        /// The first range.
        field first: bytes(current_pos..current_pos + length) { position: 2; }
        /// The second range.
        field second: bytes(current_pos..current_pos + length) { position: 3; }
    }

    /// Shared-source conflicts follow range declaration order.
    pub layout SharedConflictOrder {
        /// The source whose conflict is declared later.
        field source_a: U8 { position: 1; }
        /// The source whose conflict is declared first.
        field source_b: U8 { position: 2; }
        /// The first range for the second source.
        field b_first: bytes(current_pos..current_pos + source_b) { position: 3; }
        /// The first range for the first source.
        field a_first: bytes(current_pos..current_pos + source_a) { position: 4; }
        /// The first conflicting range in declaration order.
        field b_second: bytes(current_pos..current_pos + source_b) { position: 5; }
        /// A later conflicting range.
        field a_second: bytes(current_pos..current_pos + source_a) { position: 6; }
    }

    /// Inputs declared separately from their physical order.
    pub layout MissingOrder {
        /// Declared first but physical third.
        field later: codec(MissingCount) { position: 3; }
        /// The auto-derived length.
        field length: U8 { position: 1; }
        /// Declared before the final ordinary codec.
        field body: bytes(current_pos..current_pos + length) { position: 2; }
        /// Declared last and physical fourth.
        field final_field: codec(MissingCount) { position: 4; }
    }

    /// An invalid-width field physically before all user inputs.
    pub layout ZeroWidthBeforeInputs {
        /// The invalid codec.
        field zero: codec(ZeroWidth) { position: 1; }
        /// A later required field.
        field required: U8 { position: 2; }
        /// Keeps this scenario on the dynamic builder path.
        field dynamic: prefix(TwoBytePrefix) { position: 3; }
    }

    /// A range whose source cannot represent all lengths.
    pub layout ReverseFailure {
        /// The auto-derived narrow length.
        field length: U8 { position: 1; }
        /// The large range.
        field body: bytes(current_pos..current_pos + length) { position: 2; }
    }

    /// A planning-error layout.
    pub layout OrdinaryPlanningFailure {
        /// A rejecting field.
        field value: codec(Failing) { position: 1; }
        /// Keeps this scenario on the dynamic builder path.
        field dynamic: prefix(TwoBytePrefix) { position: 2; }
    }


    /// A fixed plan-length error layout.
    pub layout WrongFixedPlanning {
        /// A contract-invalid fixed plan.
        field value: codec(WrongFixedPlan) { position: 1; }
        /// Keeps this scenario on the dynamic builder path.
        field dynamic: prefix(TwoBytePrefix) { position: 2; }
    }

    /// A prefix plan-length error layout.
    pub layout EmptyPrefixPlanning {
        /// A contract-invalid empty prefix plan.
        field value: prefix(EmptyPrefixPlan) { position: 1; }
    }

    /// A layout containing an explicitly law-violating error fixture.
    pub layout OverflowAfterPrefix {
        /// The error-fixture prefix.
        field prefix: prefix(OverflowingPrefixPlan) { position: 1; }
        /// The physical advance after it.
        field tail: U8 { position: 2; }
    }

    /// A capacity preflight layout.
    pub layout CapacityCheck {
        /// First planned field.
        field first: codec(CapacityFirst) { position: 1; }
        /// Second planned field.
        field second: codec(CapacitySecond) { position: 2; }
        /// Keeps this scenario on the dynamic builder path.
        field dynamic: prefix(TwoBytePrefix) { position: 3; }
    }

    /// A layout whose declaration and commit orders differ.
    pub layout CommitOrder {
        /// Planned first, physically second.
        field first: codec(First) { position: 2; }
        /// Planned second, physically first.
        field second: codec(Second) { position: 1; }
        /// Keeps this scenario on the dynamic builder path.
        field dynamic: prefix(TwoBytePrefix) { position: 3; }
    }

    /// A builder with only a range input.
    pub layout RangeOnly {
        /// The auto-derived length.
        field length: U8 { position: 1; }
        /// The sole user input.
        field body: bytes(current_pos..current_pos + length) { position: 2; }
    }
}

#[test]
fn stem_builder_commits_the_mixed_dynamic_layout_atomically() {
    let mut output = [
        0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe, 0x99, 0x88, 0x77, 0x66, 0x55,
    ];
    let (mut view, suffix) = StemBuilder::new()
        .tail(0x1234)
        .tag(0xa1)
        .prefix(0x55)
        .body(&[0xaa, 0xbb])
        .build_into(&mut output)
        .unwrap();
    assert_eq!(
        view.as_bytes(),
        &[2, 0xa1, 0xf0, 0x55, 0xaa, 0xbb, 0x99, 0x88, 0x12, 0x34]
    );
    assert_eq!(view.length(), 2);
    assert_eq!(view.tag(), 0xa1);
    assert_eq!(view.prefix_raw(), &[0xf0, 0x55]);
    assert_eq!(view.prefix(), 0x55);
    assert_eq!(view.body(), &[0xaa, 0xbb]);
    assert_eq!(view.tail(), 0x1234);
    view.set_tag(0xa2).unwrap();
    assert_eq!(
        view.as_bytes(),
        &[2, 0xa2, 0xf0, 0x55, 0xaa, 0xbb, 0x99, 0x88, 0x12, 0x34]
    );
    assert_eq!(suffix, &[0x55]);
}

#[test]
fn fixed_source_derives_an_empty_range() {
    let mut output = [0xcc, 0xdd];
    let (view, suffix) = EmptyRangeBuilder::new()
        .body(&[])
        .build_into(&mut output)
        .unwrap();
    assert_eq!(view.as_bytes(), &[0]);
    assert_eq!(view.length(), 0);
    assert_eq!(view.body(), &[]);
    assert_eq!(suffix, &[0xdd]);
}

#[test]
fn shared_ranges_plan_once_or_reject_before_planning() {
    let mut output = [0; 7];
    let (view, suffix) = SharedRangesBuilder::new()
        .first(&[1, 2])
        .second(&[3, 4])
        .build_into(&mut output)
        .unwrap();
    assert_eq!(view.as_bytes(), &[2, 1, 2, 3, 4]);
    assert_eq!(suffix, &[0, 0]);

    let initial = [0xa5; 7];
    let mut output = initial;
    assert!(matches!(
        SharedRangesBuilder::new()
            .first(&[1])
            .second(&[2, 3])
            .build_into(&mut output),
        Err(SharedRangesWriteError::ConflictingRangeSources {
            source_position: 1,
            first_range_position: 2,
            conflicting_range_position: 3,
            expected: 1,
            actual: 2,
        })
    ));
    assert_eq!(output, initial);
}

#[test]
fn shared_source_conflicts_follow_range_declaration_order() {
    let initial = [0x3c; 10];
    let mut output = initial;
    assert!(matches!(
        SharedConflictOrderBuilder::new()
            .b_first(&[1])
            .a_first(&[2])
            .b_second(&[3, 4])
            .a_second(&[5, 6, 7])
            .build_into(&mut output),
        Err(SharedConflictOrderWriteError::ConflictingRangeSources {
            source_position: 2,
            first_range_position: 3,
            conflicting_range_position: 5,
            expected: 1,
            actual: 2,
        })
    ));
    assert_eq!(output, initial);
}

#[test]
fn missing_inputs_follow_declaration_order_without_planning() {
    MISSING_PLANS.store(0, Ordering::Relaxed);
    let initial = [0x5a; 4];
    let mut output = initial;
    assert!(matches!(
        MissingOrderBuilder::new().build_into(&mut output),
        Err(MissingOrderWriteError::MissingField { field: "later" })
    ));
    assert_eq!(MISSING_PLANS.load(Ordering::Relaxed), 0);
    assert_eq!(output, initial);
}

#[test]
fn zero_width_precedes_missing_inputs_and_planning() {
    ZERO_WIDTH_PLANS.store(0, Ordering::Relaxed);
    let initial = [0xa5; 2];
    let mut output = initial;
    assert!(matches!(
        ZeroWidthBeforeInputsBuilder::new().build_into(&mut output),
        Err(ZeroWidthBeforeInputsWriteError::InvalidCodecWidth { position: 1 })
    ));
    assert_eq!(ZERO_WIDTH_PLANS.load(Ordering::Relaxed), 0);
    assert_eq!(output, initial);
}

#[test]
fn unrepresentable_derived_range_length_is_atomic() {
    let initial = [0x6c; 257];
    let mut output = initial;
    assert!(matches!(
        ReverseFailureBuilder::new()
            .body(&[0; 256])
            .build_into(&mut output),
        Err(ReverseFailureWriteError::InvalidRangeSource {
            position: 2,
            source_position: 1,
            value: 256,
        })
    ));
    assert_eq!(output, initial);
}

#[test]
fn planning_failures_and_plan_lengths_leave_output_unchanged() {
    let initial = [0x44; 2];
    let mut output = initial;
    assert!(matches!(
        OrdinaryPlanningFailureBuilder::new()
            .value(1)
            .dynamic(0)
            .build_into(&mut output),
        Err(OrdinaryPlanningFailureWriteError::FieldValue(
            PlanError::Rejected
        ))
    ));
    assert_eq!(output, initial);
    assert!(matches!(
        WrongFixedPlanningBuilder::new()
            .value(1)
            .dynamic(0)
            .build_into(&mut output),
        Err(WrongFixedPlanningWriteError::InvalidPlanLength {
            field: "value",
            expected: 2,
            actual: 1,
        })
    ));
    assert_eq!(output, initial);
    assert!(matches!(
        EmptyPrefixPlanningBuilder::new()
            .value(())
            .build_into(&mut output),
        Err(EmptyPrefixPlanningWriteError::InvalidPrefixPlanLength { field: "value" })
    ));
    assert_eq!(output, initial);
}

#[test]
fn law_violating_prefix_plan_cannot_overflow_layout_extent() {
    let initial = [0x9c; 1];
    let mut output = initial;
    assert!(matches!(
        OverflowAfterPrefixBuilder::new()
            .prefix(())
            .tail(1)
            .build_into(&mut output),
        Err(OverflowAfterPrefixWriteError::InvalidLayoutExtent {
            position: 2,
            offset: usize::MAX,
            advance: 1,
        })
    ));
    assert_eq!(output, initial);
}

#[test]
fn capacity_failure_happens_after_each_plan_and_before_every_write() {
    CAPACITY_FIRST_PLANS.store(0, Ordering::Relaxed);
    CAPACITY_SECOND_PLANS.store(0, Ordering::Relaxed);
    CAPACITY_WRITES.store(0, Ordering::Relaxed);
    let initial = [0x77; 1];
    let mut output = initial;
    assert!(matches!(
        CapacityCheckBuilder::new()
            .first(1)
            .second(2)
            .dynamic(0)
            .build_into(&mut output),
        Err(CapacityCheckWriteError::OutputTooShort {
            expected: 4,
            actual: 1
        })
    ));
    assert_eq!(CAPACITY_FIRST_PLANS.load(Ordering::Relaxed), 1);
    assert_eq!(CAPACITY_SECOND_PLANS.load(Ordering::Relaxed), 1);
    assert_eq!(CAPACITY_WRITES.load(Ordering::Relaxed), 0);
    assert_eq!(output, initial);
}

#[test]
fn plans_follow_declarations_and_writes_follow_physical_order() {
    ORDER_SEQUENCE.store(0, Ordering::Relaxed);
    FIRST_PLAN_ORDER.store(usize::MAX, Ordering::Relaxed);
    SECOND_PLAN_ORDER.store(usize::MAX, Ordering::Relaxed);
    FIRST_WRITE_ORDER.store(usize::MAX, Ordering::Relaxed);
    SECOND_WRITE_ORDER.store(usize::MAX, Ordering::Relaxed);
    let mut output = [0; 4];
    let (view, suffix) = CommitOrderBuilder::new()
        .first(())
        .second(())
        .dynamic(0x33)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(FIRST_PLAN_ORDER.load(Ordering::Relaxed), 0);
    assert_eq!(SECOND_PLAN_ORDER.load(Ordering::Relaxed), 1);
    assert_eq!(SECOND_WRITE_ORDER.load(Ordering::Relaxed), 2);
    assert_eq!(FIRST_WRITE_ORDER.load(Ordering::Relaxed), 3);
    assert_eq!(view.as_bytes(), &[2, 1, 0xf0, 0x33]);
    assert!(suffix.is_empty());
}

#[test]
fn range_only_builder_omits_its_auto_source_input() {
    let mut output = [0; 4];
    let (view, suffix) = RangeOnlyBuilder::new()
        .body(&[7, 8, 9])
        .build_into(&mut output)
        .unwrap();
    assert_eq!(view.as_bytes(), &[3, 7, 8, 9]);
    assert_eq!(view.length(), 3);
    assert_eq!(view.body(), &[7, 8, 9]);
    assert!(suffix.is_empty());
}
