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

#[derive(Debug, Eq, PartialEq)]
pub enum DeriveFailure {
    Rejected,
}

fn derive_total(options_length: &u8, payload: usize) -> Result<u8, DeriveFailure> {
    u8::try_from(usize::from(*options_length) + payload).map_err(|_| DeriveFailure::Rejected)
}

fn derive_chain(total: &u8, tag: &u8) -> Result<u8, DeriveFailure> {
    total.checked_add(*tag).ok_or(DeriveFailure::Rejected)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MappedDerived(u8);

impl From<u8> for MappedDerived {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

impl From<MappedDerived> for u8 {
    fn from(value: MappedDerived) -> Self {
        value.0
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum MappedDeriveFailure {
    Rejected,
}

fn derive_mapped(tag: &u8) -> Result<MappedDerived, MappedDeriveFailure> {
    Ok(MappedDerived(*tag + 1))
}

fn derive_from_mapped(mapped: &MappedDerived) -> Result<u8, MappedDeriveFailure> {
    Ok(mapped.0 + 1)
}

pub struct NonDebugDeriveFailure;

fn reject_non_debug_derive(_: &u8) -> Result<u8, NonDebugDeriveFailure> {
    Err(NonDebugDeriveFailure)
}

fn derive_fixed_only(tag: &u8) -> Result<u8, Infallible> {
    Ok(*tag + 1)
}

fn derive_wrong_fixed_plan(_: &u8) -> Result<u8, Infallible> {
    Ok(0x44)
}

pub fn finalize_context_checksum(own: &[u8], seed: &[u8]) -> u16 {
    u16::from_be_bytes([
        seed.first()
            .copied()
            .unwrap_or(0)
            .wrapping_add(own.first().copied().unwrap_or(0)),
        seed.get(1)
            .copied()
            .unwrap_or(0)
            .wrapping_add(own.get(1).copied().unwrap_or(0)),
    ])
}

pub fn finalize_first_patch(_: &[u8]) -> u8 {
    0x11
}

pub fn finalize_second_patch(bytes: &[u8]) -> u8 {
    bytes.first().copied().unwrap_or(0).wrapping_add(0x11)
}

pub fn finalize_later_value(_: &[u8]) -> i8 {
    -3
}

pub fn finalize_earlier_value(later: &i8) -> i8 {
    later.wrapping_add(4)
}

pub fn finalize_represented_length(bytes: &[u8]) -> u8 {
    bytes.len() as u8
}

pub fn finalize_existing_sum(bytes: &[u8]) -> u8 {
    bytes.iter().copied().fold(0, u8::wrapping_add)
}

pub fn finalize_value_sources(ordinary: &u8, mapped: &MappedDerived, derived: &u8) -> u16 {
    u16::from_be_bytes([ordinary.wrapping_add(*derived), mapped.0])
}

pub fn finalize_u8(_: &[u8]) -> u8 {
    0xa5
}
pub fn finalize_i8(_: &[u8]) -> i8 {
    -91
}
pub fn finalize_be_u16(_: &[u8]) -> u16 {
    0x1234
}
pub fn finalize_le_u16(_: &[u8]) -> u16 {
    0x1234
}
pub fn finalize_be_i16(_: &[u8]) -> i16 {
    -0x1234
}
pub fn finalize_le_i16(_: &[u8]) -> i16 {
    -0x1234
}
pub fn finalize_be_u32(_: &[u8]) -> u32 {
    0x1234_5678
}
pub fn finalize_le_u32(_: &[u8]) -> u32 {
    0x1234_5678
}
pub fn finalize_be_i32(_: &[u8]) -> i32 {
    -0x0123_4567
}
pub fn finalize_le_i32(_: &[u8]) -> i32 {
    -0x0123_4567
}
pub fn finalize_be_u64(_: &[u8]) -> u64 {
    0x0123_4567_89ab_cdef
}
pub fn finalize_le_u64(_: &[u8]) -> u64 {
    0x0123_4567_89ab_cdef
}
pub fn finalize_be_i64(_: &[u8]) -> i64 {
    -0x0012_3456_789a_bcde
}
pub fn finalize_le_i64(_: &[u8]) -> i64 {
    -0x0012_3456_789a_bcde
}
pub fn finalize_be_u128(_: &[u8]) -> u128 {
    0x0123_4567_89ab_cdef_1020_3040_5060_7080
}
pub fn finalize_le_u128(_: &[u8]) -> u128 {
    0x0123_4567_89ab_cdef_1020_3040_5060_7080
}
pub fn finalize_be_i128(_: &[u8]) -> i128 {
    -0x0012_3456_789a_bcde_f102_0304_0506_0708
}
pub fn finalize_le_i128(_: &[u8]) -> i128 {
    -0x0012_3456_789a_bcde_f102_0304_0506_0708
}

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


    /// Relative range assembly with caller-retained bytes.
    pub layout ExistingRelative {
        field length: U8 { position: 1; }
        field body: bytes(current_pos..current_pos + length) { position: 2; }
        field tail: U8 { position: 3; }
    }

    /// Absolute range assembly with caller-retained bytes.
    pub layout ExistingAbsolute {
        field end: U8 { position: 1; }
        field body: bytes(current_pos..end) { position: 2; }
    }

    /// Absolute intermediate range assembly with caller-retained bytes.
    pub layout ExistingAbsoluteIntermediate {
        field end: U8 { position: 1; }
        field body: bytes(current_pos..end) { position: 2; }
        padding { position: 3; length: 1; }
        field tail: U8 { position: 4; }
    }

    /// Explicit pre-write derivations consume borrowed and existing range lengths.
    pub layout DerivedAssembly {
        field tag: U8 { position: 1; }
        field options_length: U8 { position: 2; }
        field options: bytes(current_pos..current_pos + options_length) { position: 3; }
        field payload_length: U8 { position: 4; }
        field payload: bytes(current_pos..current_pos + payload_length) { position: 5; }
        field total: U8 {
            position: 6;
            derive: crate::derive_total(value(options_length), len(payload));
            derive_error: crate::DeriveFailure;
        }
        field chain: U8 {
            position: 7;
            derive: crate::derive_chain(value(total), value(tag));
            derive_error: crate::DeriveFailure;
        }
    }

    /// Terminal range assembly with caller-retained bytes.
    pub layout ExistingTerminal {
        field header: U8 { position: 1; }
        field body: bytes(current_pos..buf_end) { position: 2; }
    }


    /// Explicit mapped derivations preserve semantic values through chains.
    pub layout MappedDerivedAssembly {
        field tag: U8 { position: 1; }
        field mapped_derived: U8 as crate::MappedDerived {
            position: 2;
            derive: crate::derive_mapped(value(tag));
            derive_error: crate::MappedDeriveFailure;
        }
        field chained: U8 {
            position: 3;
            derive: crate::derive_from_mapped(value(mapped_derived));
            derive_error: crate::MappedDeriveFailure;
        }
    }

    /// A derived error need not implement Debug for the generated Display path.
    pub layout NonDebugDerived {
        field tag: U8 { position: 1; }
        field derived: U8 {
            position: 2;
            derive: crate::reject_non_debug_derive(value(tag));
            derive_error: crate::NonDebugDeriveFailure;
        }
    }

    /// A successful derivation can still fail fixed-codec planning atomically.
    pub layout DerivedWrongFixedPlan {
        field tag: U8 { position: 1; }
        field derived: codec(WrongFixedPlan) {
            position: 2;
            derive: crate::derive_wrong_fixed_plan(value(tag));
            derive_error: core::convert::Infallible;
        }
    }

    /// A fixed-only dynamic builder whose derived field needs no input.
    pub layout FixedOnlyDerived {
        field tag: U8 { position: 1; }
        field derived: U8 {
            position: 2;
            derive: crate::derive_fixed_only(value(tag));
            derive_error: core::convert::Infallible;
        }
    }

    /// A borrowed unsized context drives a final checksum after ordinary writes.
    pub layout ContextFinalization {
        context seed: [u8];
        field tag: U8 { position: 1; }
        field checksum: BeU16 {
            position: 2;
            finalize: crate::finalize_context_checksum(bytes(checksum.start..checksum.end), context(seed));
        }
    }

    /// Independent finalizers preserve declaration order even when spans overlap.
    pub layout StableFinalizerOrder {
        field first: U8 {
            position: 1;
            finalize: crate::finalize_first_patch(bytes(buf_start..buf_end));
        }
        field second: U8 {
            position: 2;
            finalize: crate::finalize_second_patch(bytes(buf_start..buf_end));
        }
    }

    /// Finalizers consume ordinary, mapped, and pre-write-derived semantic values.
    pub layout SemanticValueFinalization {
        field ordinary: U8 { position: 1; }
        field mapped: U8 as crate::MappedDerived { position: 2; }
        field derived: U8 {
            position: 3;
            derive: crate::derive_fixed_only(value(ordinary));
            derive_error: core::convert::Infallible;
        }
        field checksum: BeU16 {
            position: 4;
            finalize: crate::finalize_value_sources(value(ordinary), value(mapped), value(derived));
        }
    }

    /// An explicit finalizer value dependency may point forward in declarations.
    pub layout ForwardFinalizerValue {
        field earlier: I8 {
            position: 1;
            finalize: crate::finalize_earlier_value(value(later));
        }
        field later: I8 {
            position: 2;
            finalize: crate::finalize_later_value(bytes(buf_start..buf_start));
        }
    }

    /// `buf_end` stops at the represented layout, not the caller's whole output.
    pub layout RepresentedFinalizerExtent {
        field header: U8 { position: 1; }
        field length: U8 {
            position: 2;
            finalize: crate::finalize_represented_length(bytes(buf_start..buf_end));
        }
    }

    /// Existing destination bytes remain available to post-write finalizers.
    pub layout ExistingFinalizerSpan {
        field length: U8 { position: 1; }
        field body: bytes(current_pos..current_pos + length) { position: 2; }
        field checksum: U8 {
            position: 3;
            finalize: crate::finalize_existing_sum(bytes(body.start..checksum.end));
        }
    }

    /// One finalizer target covers each direct builtin patch encoding.
    pub layout FinalizerBuiltinEncodings {
        field u8: U8 { position: 1; finalize: crate::finalize_u8(bytes(buf_start..buf_start)); }
        field i8: I8 { position: 2; finalize: crate::finalize_i8(bytes(buf_start..buf_start)); }
        field be_u16: BeU16 { position: 3; finalize: crate::finalize_be_u16(bytes(buf_start..buf_start)); }
        field le_u16: LeU16 { position: 4; finalize: crate::finalize_le_u16(bytes(buf_start..buf_start)); }
        field be_i16: BeI16 { position: 5; finalize: crate::finalize_be_i16(bytes(buf_start..buf_start)); }
        field le_i16: LeI16 { position: 6; finalize: crate::finalize_le_i16(bytes(buf_start..buf_start)); }
        field be_u32: BeU32 { position: 7; finalize: crate::finalize_be_u32(bytes(buf_start..buf_start)); }
        field le_u32: LeU32 { position: 8; finalize: crate::finalize_le_u32(bytes(buf_start..buf_start)); }
        field be_i32: BeI32 { position: 9; finalize: crate::finalize_be_i32(bytes(buf_start..buf_start)); }
        field le_i32: LeI32 { position: 10; finalize: crate::finalize_le_i32(bytes(buf_start..buf_start)); }
        field be_u64: BeU64 { position: 11; finalize: crate::finalize_be_u64(bytes(buf_start..buf_start)); }
        field le_u64: LeU64 { position: 12; finalize: crate::finalize_le_u64(bytes(buf_start..buf_start)); }
        field be_i64: BeI64 { position: 13; finalize: crate::finalize_be_i64(bytes(buf_start..buf_start)); }
        field le_i64: LeI64 { position: 14; finalize: crate::finalize_le_i64(bytes(buf_start..buf_start)); }
        field be_u128: BeU128 { position: 15; finalize: crate::finalize_be_u128(bytes(buf_start..buf_start)); }
        field le_u128: LeU128 { position: 16; finalize: crate::finalize_le_u128(bytes(buf_start..buf_start)); }
        field be_i128: BeI128 { position: 17; finalize: crate::finalize_be_i128(bytes(buf_start..buf_start)); }
        field le_i128: LeI128 { position: 18; finalize: crate::finalize_le_i128(bytes(buf_start..buf_start)); }
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

#[test]
fn intermediate_absolute_ranges_derive_endpoints_and_preserve_exact_spans() {
    let mut borrowed = [0xa5; 7];
    let (view, suffix) = ExistingAbsoluteIntermediateBuilder::new()
        .body(&[0x10, 0x20])
        .tail(0x33)
        .build_into(&mut borrowed)
        .unwrap();
    assert_eq!(view.as_bytes(), &[3, 0x10, 0x20, 0xa5, 0x33]);
    assert_eq!(view.body(), &[0x10, 0x20]);
    assert_eq!(view.tail(), 0x33);
    assert_eq!(suffix, &[0xa5, 0xa5]);

    let mut existing = [0xa5, 0x41, 0x42, 0xa5, 0xa5, 0xa5];
    let (view, suffix) = ExistingAbsoluteIntermediateBuilder::new()
        .body_existing(2)
        .tail(0x44)
        .build_into(&mut existing)
        .unwrap();
    assert_eq!(view.as_bytes(), &[3, 0x41, 0x42, 0xa5, 0x44]);
    assert_eq!(view.body(), &[0x41, 0x42]);
    assert_eq!(suffix, &[0xa5]);
    assert_eq!(existing, [3, 0x41, 0x42, 0xa5, 0x44, 0xa5]);

    let initial = [0x7c; 4];
    let mut short = initial;
    assert!(matches!(
        ExistingAbsoluteIntermediateBuilder::new()
            .body_existing(2)
            .tail(0x44)
            .build_into(&mut short),
        Err(ExistingAbsoluteIntermediateWriteError::OutputTooShort {
            expected: 5,
            actual: 4,
        })
    ));
    assert_eq!(short, initial);
}

#[test]
fn existing_ranges_bound_layout_derive_sources_and_never_write_their_spans() {
    let mut relative = [0xa5, 0x10, 0x20, 0xa5, 0xa5];
    let (view, suffix) = ExistingRelativeBuilder::new()
        .body_existing(2)
        .tail(0x30)
        .build_into(&mut relative)
        .unwrap();
    assert_eq!(view.as_bytes(), &[2, 0x10, 0x20, 0x30]);
    assert_eq!(view.body(), &[0x10, 0x20]);
    assert_eq!(view.tail(), 0x30);
    assert_eq!(suffix, &[0xa5]);
    assert_eq!(relative, [2, 0x10, 0x20, 0x30, 0xa5]);

    let mut absolute = [0xa5, 0x10, 0x20, 0xa5, 0xa5];
    let (view, suffix) = ExistingAbsoluteBuilder::new()
        .body_existing(2)
        .build_into(&mut absolute)
        .unwrap();
    assert_eq!(view.as_bytes(), &[3, 0x10, 0x20]);
    assert_eq!(suffix, &[0xa5, 0xa5]);
    assert_eq!(absolute, [3, 0x10, 0x20, 0xa5, 0xa5]);

    let mut terminal = [0xa5, 0x10, 0x20, 0xa5, 0xa5];
    let (view, suffix) = ExistingTerminalBuilder::new()
        .header(0x55)
        .body_existing(2)
        .build_into(&mut terminal)
        .unwrap();
    assert_eq!(view.as_bytes(), &[0x55, 0x10, 0x20]);
    assert_eq!(suffix, &[0xa5, 0xa5]);
    assert_eq!(terminal, [0x55, 0x10, 0x20, 0xa5, 0xa5]);

    let initial = [0x7c; 3];
    let mut short = initial;
    assert!(matches!(
        ExistingTerminalBuilder::new()
            .header(1)
            .body_existing(3)
            .build_into(&mut short),
        Err(ExistingTerminalWriteError::OutputTooShort {
            expected: 4,
            actual: 3
        })
    ));
    assert_eq!(short, initial);
}

#[test]
fn explicit_derived_fields_preflight_borrowed_existing_and_chained_inputs() {
    let mut output = [0xa5; 10];
    let (view, suffix) = DerivedAssemblyBuilder::new()
        .tag(4)
        .options(&[0x10, 0x20])
        .payload_existing(3)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(
        view.as_bytes(),
        &[4, 2, 0x10, 0x20, 3, 0xa5, 0xa5, 0xa5, 5, 9]
    );
    assert!(suffix.is_empty());
    assert_eq!(output, [4, 2, 0x10, 0x20, 3, 0xa5, 0xa5, 0xa5, 5, 9]);
}

#[test]
fn explicit_derive_failure_is_atomic() {
    let initial = [0xa5; 300];
    let mut output = initial;
    assert!(matches!(
        DerivedAssemblyBuilder::new()
            .tag(1)
            .options(&[0; 255])
            .payload_existing(1)
            .build_into(&mut output),
        Err(DerivedAssemblyWriteError::DeriveFieldTotal(
            DeriveFailure::Rejected
        ))
    ));
    assert_eq!(output, initial);
}

#[test]
fn mapped_explicit_derivations_chain_through_semantic_values() {
    let mut output = [0xa5; 4];
    let (view, suffix) = MappedDerivedAssemblyBuilder::new()
        .tag(0x10)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(view.as_bytes(), &[0x10, 0x11, 0x12]);
    assert_eq!(view.tag(), 0x10);
    assert_eq!(view.mapped_derived(), MappedDerived(0x11));
    assert_eq!(view.mapped_derived_raw(), 0x11);
    assert_eq!(view.chained(), 0x12);
    assert_eq!(suffix, &[0xa5]);
}

#[test]
fn non_debug_derive_errors_still_support_generated_display() {
    let mut output = [0xa5; 2];
    let error = match NonDebugDerivedBuilder::new().tag(1).build_into(&mut output) {
        Err(error) => error,
        Ok(_) => panic!("derivation should fail"),
    };
    assert!(matches!(
        &error,
        NonDebugDerivedWriteError::DeriveFieldDerived(NonDebugDeriveFailure)
    ));
    let _: &dyn core::error::Error = &error;
    assert_eq!(
        format!("{error:?}"),
        "DeriveFieldDerived { payload: \"<opaque derivation error>\" }"
    );
    assert_eq!(error.to_string(), "field derived failed derivation");
}

#[test]
fn derived_codec_plan_failures_are_atomic() {
    let initial = [0xa5; 2];
    let mut output = initial;
    assert!(matches!(
        DerivedWrongFixedPlanBuilder::new()
            .tag(1)
            .build_into(&mut output),
        Err(DerivedWrongFixedPlanWriteError::InvalidPlanLength {
            field: "derived",
            expected: 2,
            actual: 1,
        })
    ));
    assert_eq!(output, initial);
}

#[test]
fn fixed_only_derived_layout_needs_only_ordinary_inputs() {
    let mut output = [0xa5; 3];
    let (view, suffix) = FixedOnlyDerivedBuilder::new()
        .tag(0x20)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(view.as_bytes(), &[0x20, 0x21]);
    assert_eq!(view.tag(), 0x20);
    assert_eq!(view.derived(), 0x21);
    assert_eq!(suffix, &[0xa5]);
}

#[test]
fn contexts_are_required_borrowed_and_finalizers_observe_zeroed_targets() {
    let initial = [0xde, 0xad, 0xbe, 0xef];
    let mut missing = initial;
    let error = match ContextFinalizationBuilder::new()
        .tag(0x44)
        .build_into(&mut missing)
    {
        Err(error) => error,
        Ok(_) => panic!("context should be required"),
    };
    assert!(matches!(
        error,
        ContextFinalizationWriteError::MissingContext { context: "seed" }
    ));
    assert_eq!(error.to_string(), "missing context seed");
    assert_eq!(missing, initial);

    let seed: &[u8] = &[0x12, 0x34];
    let mut output = [0xff, 0xee, 0xdd, 0xcc];
    let (view, suffix) = ContextFinalizationBuilder::new()
        .tag(0x44)
        .seed(seed)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(view.as_bytes(), &[0x44, 0x12, 0x34]);
    assert_eq!(view.tag(), 0x44);
    assert_eq!(view.checksum(), 0x1234);
    assert_eq!(suffix, &[0xcc]);
}

#[test]
fn independent_finalizers_follow_declaration_order_and_observe_prior_patches() {
    let mut output = [0xfe; 3];
    let (view, suffix) = StableFinalizerOrderBuilder::new()
        .build_into(&mut output)
        .unwrap();
    assert_eq!(view.as_bytes(), &[0x11, 0x22]);
    assert_eq!(view.first(), 0x11);
    assert_eq!(view.second(), 0x22);
    assert_eq!(suffix, &[0xfe]);
}

#[test]
fn finalizers_consume_ordinary_mapped_and_derived_semantic_values() {
    let mut output = [0xfe; 6];
    let (view, suffix) = SemanticValueFinalizationBuilder::new()
        .ordinary(0x10)
        .mapped(MappedDerived(0x20))
        .build_into(&mut output)
        .unwrap();
    assert_eq!(view.as_bytes(), &[0x10, 0x20, 0x11, 0x21, 0x20]);
    assert_eq!(view.ordinary(), 0x10);
    assert_eq!(view.mapped(), MappedDerived(0x20));
    assert_eq!(view.mapped_raw(), 0x20);
    assert_eq!(view.derived(), 0x11);
    assert_eq!(view.checksum(), 0x2120);
    assert_eq!(suffix, &[0xfe]);
}

#[test]
fn forward_finalizer_value_dependencies_reorder_calls_with_semantic_references() {
    let mut output = [0xfe; 3];
    let (view, suffix) = ForwardFinalizerValueBuilder::new()
        .build_into(&mut output)
        .unwrap();
    assert_eq!(view.as_bytes(), &[1, 0xfd]);
    assert_eq!(view.earlier(), 1);
    assert_eq!(view.later(), -3);
    assert_eq!(suffix, &[0xfe]);
}

#[test]
fn finalizer_buf_end_excludes_the_untouched_output_suffix() {
    let mut output = [0xfe; 5];
    let (view, suffix) = RepresentedFinalizerExtentBuilder::new()
        .header(0x44)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(view.as_bytes(), &[0x44, 2]);
    assert_eq!(view.header(), 0x44);
    assert_eq!(view.length(), 2);
    assert_eq!(suffix, &[0xfe, 0xfe, 0xfe]);
}

#[test]
fn finalizer_spans_include_existing_ranges_without_rewriting_them() {
    let mut output = [0xfe, 0x10, 0x20, 0xfe, 0xfe];
    let (view, suffix) = ExistingFinalizerSpanBuilder::new()
        .body_existing(2)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(view.as_bytes(), &[2, 0x10, 0x20, 0x30]);
    assert_eq!(view.body(), &[0x10, 0x20]);
    assert_eq!(view.checksum(), 0x30);
    assert_eq!(suffix, &[0xfe]);
    assert_eq!(output, [2, 0x10, 0x20, 0x30, 0xfe]);
}

#[test]
fn finalizers_patch_every_supported_builtin_encoding_exactly() {
    let mut output = [0; 122];
    let (view, suffix) = FinalizerBuiltinEncodingsBuilder::new()
        .build_into(&mut output)
        .unwrap();
    assert_eq!(
        view.as_bytes(),
        &[
            0xa5, 0xa5, 0x12, 0x34, 0x34, 0x12, 0xed, 0xcc, 0xcc, 0xed, 0x12, 0x34, 0x56, 0x78,
            0x78, 0x56, 0x34, 0x12, 0xfe, 0xdc, 0xba, 0x99, 0x99, 0xba, 0xdc, 0xfe, 0x01, 0x23,
            0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01,
            0xff, 0xed, 0xcb, 0xa9, 0x87, 0x65, 0x43, 0x22, 0x22, 0x43, 0x65, 0x87, 0xa9, 0xcb,
            0xed, 0xff, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x10, 0x20, 0x30, 0x40,
            0x50, 0x60, 0x70, 0x80, 0x80, 0x70, 0x60, 0x50, 0x40, 0x30, 0x20, 0x10, 0xef, 0xcd,
            0xab, 0x89, 0x67, 0x45, 0x23, 0x01, 0xff, 0xed, 0xcb, 0xa9, 0x87, 0x65, 0x43, 0x21,
            0x0e, 0xfd, 0xfc, 0xfb, 0xfa, 0xf9, 0xf8, 0xf8, 0xf8, 0xf8, 0xf9, 0xfa, 0xfb, 0xfc,
            0xfd, 0x0e, 0x21, 0x43, 0x65, 0x87, 0xa9, 0xcb, 0xed, 0xff,
        ]
    );
    assert_eq!(view.u8(), 0xa5);
    assert_eq!(view.i8(), -91);
    assert!(suffix.is_empty());
}
