#![deny(missing_docs, unsafe_code)]
//! Computed byte-length field coverage.

use wire_repr::{ByteSourceCursor, FixedCodec, PreparedLayout, Wire};

/// A packet with a computed bounded payload length.
#[derive(Wire)]
pub struct Packet<'wire> {
    /// Payload byte count.
    #[wire(computed = wire_repr::computation::len(payload))]
    pub length: u8,
    /// Borrowed payload.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
    /// Packet kind.
    pub kind: u8,
}

/// A big-endian computed 16-bit length.
#[derive(Wire)]
pub struct BigEndianPacket<'wire> {
    /// Payload byte count.
    #[wire(computed = wire_repr::computation::len(payload), be)]
    pub length: u16,
    /// Borrowed payload.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
}

/// A little-endian computed 16-bit length.
#[derive(Wire)]
pub struct LittleEndianPacket<'wire> {
    /// Payload byte count.
    #[wire(computed = wire_repr::computation::len(payload), le)]
    pub length: u16,
    /// Borrowed payload.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
}

/// A computed field with an explicitly selected 24-bit representation.
#[derive(Wire)]
pub struct Qualified<'wire> {
    /// Payload byte count.
    #[wire(computed = wire_repr::computation::len(payload), codec = wire_repr::BeU24)]
    pub length: u32,
    /// Borrowed payload.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
}

/// A nominal payload length with an explicit one-byte representation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PayloadLength(u8);

impl TryFrom<usize> for PayloadLength {
    type Error = core::num::TryFromIntError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        Ok(Self(u8::try_from(value)?))
    }
}

impl From<PayloadLength> for usize {
    fn from(value: PayloadLength) -> Self {
        usize::from(value.0)
    }
}

/// A fixed codec preserving the nominal length type.
pub struct PayloadLengthCodec;

impl FixedCodec for PayloadLengthCodec {
    type Value<'wire> = PayloadLength;
    type EncodeError = core::convert::Infallible;
    type Plan<'value> = [u8; 1];

    const WIDTH: usize = 1;

    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        PayloadLength(bytes[0])
    }

    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([value.0])
    }
}

/// A computed field whose semantic value is a nominal type.
#[derive(Wire)]
pub struct NominalPacket<'wire> {
    /// Payload byte count.
    #[wire(computed = wire_repr::computation::len(payload), codec = PayloadLengthCodec)]
    pub length: PayloadLength,
    /// Borrowed payload.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
}

/// A packet whose remainder is length-prefixed.
#[derive(Wire)]
pub struct RestPacket<'wire> {
    /// Remainder byte count.
    #[wire(computed = wire_repr::computation::len(payload))]
    pub length: u8,
    /// Remaining bytes.
    #[wire(rest)]
    pub payload: &'wire [u8],
}

/// A borrowed nested tail used by a computed parent builder.
#[derive(Wire)]
pub struct NestedTail<'wire> {
    /// Tail kind.
    pub kind: u8,
    /// Remaining tail bytes.
    #[wire(rest)]
    pub data: &'wire [u8],
}

/// A computed parent retaining a borrowed nested semantic input.
#[derive(Wire)]
pub struct NestedPacket<'wire> {
    /// Payload byte count.
    #[wire(computed = wire_repr::computation::len(payload))]
    pub length: u8,
    /// Borrowed payload.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
    /// Borrowed nested tail.
    pub tail: NestedTail<'wire>,
}

fn byte_sum(source: &impl ByteSourceCursor) -> u8 {
    source.bytes().fold(0u8, u8::wrapping_add)
}

fn byte_count(source: &impl ByteSourceCursor) -> usize {
    source.byte_len()
}

fn oversized_count(_: &impl ByteSourceCursor) -> usize {
    256
}

fn constant_count() -> usize {
    7
}

fn semantic_count(value: &u8) -> usize {
    usize::from(*value) + 1
}

fn ordered_count(
    kind: &u8,
    first: &impl ByteSourceCursor,
    remaining: &impl ByteSourceCursor,
) -> usize {
    usize::from(*kind) * 100 + first.byte_len() * 10 + remaining.byte_len()
}

/// A computation receiving a semantic field by shared reference.
#[derive(Wire)]
pub struct SemanticCallback {
    /// Derived from the semantic value.
    #[wire(computed = semantic_count(value))]
    pub count: u8,
    /// Ordinary semantic input.
    pub value: u8,
}

/// A computation with no callback arguments.
#[derive(Wire)]
pub struct NoArgumentCallback {
    /// Constant computed value.
    #[wire(computed = constant_count())]
    pub count: u8,
}

/// An empty physical selection remains an ordinary callback argument.
#[derive(Wire)]
pub struct EmptyIncludeCallback {
    /// Number of selected bytes.
    #[wire(computed = byte_count(include()))]
    pub count: u8,
    /// Ordinary field omitted from the empty selection.
    pub value: u8,
}

/// A computation with semantic and independently selected physical arguments.
#[derive(Wire)]
pub struct OrderedCallback {
    /// Ordered callback result.
    #[wire(computed = ordered_count(kind, include(first), exclude(self, second)))]
    pub checksum: u8,
    /// Semantic callback input.
    pub kind: u8,
    /// First physical selection.
    pub first: u8,
    /// Explicitly excluded physical field.
    pub second: u8,
    /// Remaining physical bytes included by the exclusion selection.
    pub tail: u8,
}

/// Duplicate selectors are one physical set rather than an error.
#[derive(Wire)]
pub struct DuplicateSelection {
    /// Count of the selected bytes.
    #[wire(computed = byte_count(include(value, value)))]
    pub count: u8,
    /// One selected byte.
    pub value: u8,
}

/// A callback-derived count converted into its computed destination type.
#[derive(Wire)]
pub struct CallbackCount {
    /// Number of selected physical bytes.
    #[wire(computed = byte_count(include(value)))]
    pub count: u8,
    /// The selected byte.
    pub value: u8,
}

/// A callback whose result is not representable by its destination type.
#[derive(Wire)]
pub struct OversizedCallback {
    /// Callback result converted into the computed destination type.
    #[wire(computed = oversized_count(include(value)))]
    pub count: u8,
    /// The selected byte.
    pub value: u8,
}

/// A fixed-only representation with a callback over owned fields.
#[derive(Wire)]
pub struct FixedCallback {
    /// Sum of the selected physical bytes.
    #[wire(computed = byte_sum(include(first, second)))]
    pub checksum: u8,
    /// First selected byte.
    pub first: u8,
    /// Second selected byte.
    pub second: u8,
}

/// A representation containing only a self-excluding computed field.
#[derive(Wire)]
pub struct FixedExcludeSelf {
    /// Sum of all physical bytes except this field.
    #[wire(computed = byte_sum(exclude(self)))]
    pub checksum: u8,
}

/// A checksum that depends on a later computed length field.
#[derive(Wire)]
pub struct Checksummed<'wire> {
    /// Sum of every physical byte except this field.
    #[wire(computed = byte_sum(exclude(self)))]
    pub checksum: u8,
    /// Payload byte count.
    #[wire(computed = wire_repr::computation::len(payload))]
    pub length: u8,
    /// Remaining payload bytes.
    #[wire(rest)]
    pub payload: &'wire [u8],
}

/// Computed fields whose physical read-set determines preparation order.
#[derive(Wire)]
pub struct IncludedDependency<'wire> {
    /// Sum of the prepared partial checksum and payload.
    #[wire(computed = byte_sum(include(partial, payload)))]
    pub checksum: u8,
    /// Sum of the payload alone.
    #[wire(computed = byte_sum(include(payload)))]
    pub partial: u8,
    /// Remaining payload bytes.
    #[wire(rest)]
    pub payload: &'wire [u8],
}

/// A checksum selecting one projected nested field.
#[derive(Wire)]
pub struct NestedSelection<'wire> {
    /// Sum of the nested kind byte.
    #[wire(computed = byte_sum(include(tail.kind)))]
    pub checksum: u8,
    /// Nested representation.
    pub tail: NestedTail<'wire>,
}

fn source_len(source: &impl ByteSourceCursor) -> u8 {
    u8::try_from(source.byte_len()).expect("test source length fits u8")
}

/// A computation whose selected source includes physical positioning gaps.
#[derive(Wire)]
pub struct PositionedChecksum<'wire> {
    /// Length of every physical byte except this field.
    #[wire(computed = source_len(exclude(self)))]
    pub checksum: u8,
    /// A positioned byte preceded by a generated gap.
    #[wire(at = 4)]
    pub marker: u8,
    /// Trailing byte.
    pub tail: u8,
    /// Remaining bytes.
    #[wire(rest)]
    pub payload: &'wire [u8],
}

#[derive(Debug)]
enum ChecksumError {
    Decode(ValidatedChecksumDecodeError),
    Mismatch,
}

impl core::fmt::Display for ChecksumError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Decode(error) => error.fmt(formatter),
            Self::Mismatch => formatter.write_str("checksum mismatch"),
        }
    }
}

impl core::error::Error for ChecksumError {}

impl From<ValidatedChecksumDecodeError> for ChecksumError {
    fn from(error: ValidatedChecksumDecodeError) -> Self {
        Self::Decode(error)
    }
}

fn checksum_matches(view: &ValidatedChecksumView<'_>) -> Result<(), ChecksumError> {
    let expected = byte_sum(&view.bytes().exclude(|fields| fields.checksum));
    if view.checksum() == expected {
        Ok(())
    } else {
        Err(ChecksumError::Mismatch)
    }
}

#[derive(Wire)]
#[wire(error = ChecksumError, validate = checksum_matches)]
#[allow(dead_code)]
struct ValidatedChecksum<'wire> {
    #[wire(computed = byte_sum(exclude(self)))]
    checksum: u8,
    #[wire(rest)]
    payload: &'wire [u8],
}

/// Consumer-owned callback selector.
#[derive(Clone, Copy, PartialEq)]
pub enum CallbackSelector {
    /// The only selected nested variant.
    Ping,
}

/// Consumer-owned callback table failure.
#[derive(Debug)]
pub struct CallbackTableError;

impl core::fmt::Display for CallbackTableError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("callback table unavailable")
    }
}

impl core::error::Error for CallbackTableError {}

/// A consumer-owned callback table.
pub struct CallbackTable {
    tag: u8,
}

impl CallbackTable {
    fn decode(&self, tag: u8) -> Result<Option<CallbackSelector>, CallbackTableError> {
        Ok((tag == self.tag).then_some(CallbackSelector::Ping))
    }

    fn encode(&self, selector: CallbackSelector) -> Result<Option<u8>, CallbackTableError> {
        match selector {
            CallbackSelector::Ping => Ok(Some(self.tag)),
        }
    }
}

/// A fixed nested callback body.
#[derive(Wire)]
pub struct CallbackBody {
    /// Body byte.
    pub value: u8,
}

/// An operation-table-selected nested enum.
#[derive(Wire)]
#[wire(tag = U8, table = CallbackTable, table_error = CallbackTableError, unknown = reject)]
pub enum CallbackOperation {
    /// Carries a fixed body.
    #[wire(table = CallbackSelector::Ping)]
    Ping(CallbackBody),
}

/// A fixed-only computed builder with a table-selected nested operation.
#[derive(Wire)]
#[wire(table = CallbackTable)]
pub struct CallbackEnvelope {
    /// Checksum over the selected operation's exact physical bytes.
    #[wire(computed = byte_sum(include(selected)))]
    pub checksum: u8,
    /// Selected operation.
    #[wire(table)]
    pub selected: CallbackOperation,
}

/// A borrowed computed builder whose length must be prepared before geometry.
#[derive(Wire)]
#[wire(table = CallbackTable)]
pub struct CallbackPayload<'wire> {
    /// Payload byte count.
    #[wire(computed = wire_repr::computation::len(payload))]
    pub length: u8,
    /// Borrowed payload.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
    /// Selected operation.
    #[wire(table)]
    pub selected: CallbackOperation,
}

#[test]
fn builder_computes_u8_length_and_round_trips() {
    let payload = [1, 2, 3];
    let plan = Packet::builder()
        .payload(&payload)
        .kind(7)
        .prepare()
        .unwrap();
    assert_eq!(plan.encoded_len(), 5);
    let mut output = [0; 6];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[3, 1, 2, 3, 7]);
    assert_eq!(suffix, &mut [0]);
    let parsed = Packet::view(written.as_bytes()).without_trailing().unwrap();
    assert_eq!(parsed.length(), 3);
    assert_eq!(parsed.payload(), payload);
    assert_eq!(parsed.kind(), 7);
}

#[test]
fn direct_prepare_ignores_the_caller_supplied_computed_value() {
    let payload = [1, 2, 3];
    let plan = Packet {
        length: u8::MAX,
        payload: &payload,
        kind: 7,
    }
    .prepare()
    .unwrap();
    let mut output = [0; 5];

    let (written, suffix) = plan.commit_into(&mut output).unwrap();

    assert_eq!(written.as_bytes(), &[3, 1, 2, 3, 7]);
    assert!(suffix.is_empty());
}

#[test]
fn computed_u16_uses_selected_byte_order_and_raw_getter() {
    let payload = [1, 2, 3];
    let mut big = [0; 5];
    let (written, _) = BigEndianPacket::builder()
        .payload(&payload)
        .build_into(&mut big)
        .unwrap();
    assert_eq!(written.as_bytes(), &[0, 3, 1, 2, 3]);
    assert_eq!(
        BigEndianPacket::view(written.as_bytes())
            .without_trailing()
            .unwrap()
            .length(),
        3u16
    );

    let mut little = [0; 5];
    let (written, _) = LittleEndianPacket::builder()
        .payload(&payload)
        .build_into(&mut little)
        .unwrap();
    assert_eq!(written.as_bytes(), &[3, 0, 1, 2, 3]);
    assert_eq!(
        LittleEndianPacket::view(written.as_bytes())
            .without_trailing()
            .unwrap()
            .length(),
        3u16
    );
}

#[test]
fn computed_custom_codec_uses_raw_value_type() {
    let payload = [9, 8];
    let mut output = [0; 5];
    let (written, _) = Qualified::builder()
        .payload(&payload)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[0, 0, 2, 9, 8]);
    assert_eq!(
        Qualified::view(written.as_bytes())
            .without_trailing()
            .unwrap()
            .length(),
        2u32
    );
}

#[test]
fn computed_custom_codec_preserves_a_nominal_semantic_type() {
    let payload = [4, 5, 6];
    let mut output = [0; 4];
    let (written, _) = NominalPacket::builder()
        .payload(&payload)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[3, 4, 5, 6]);

    let parsed = NominalPacket::view(written.as_bytes())
        .without_trailing()
        .unwrap();
    assert_eq!(parsed.length(), PayloadLength(3));
    assert_eq!(parsed.payload(), payload);

    let oversized = [0; 256];
    assert!(matches!(
        NominalPacket::builder().payload(&oversized).prepare(),
        Err(NominalPacketEncodeError::ComputedValueNotRepresentable { field: "length" })
    ));
}

#[test]
fn builder_reports_conversion_failure_and_preserves_short_output() {
    let oversized = [0; 256];
    assert!(matches!(
        Packet::builder().payload(&oversized).kind(1).prepare(),
        Err(PacketEncodeError::ComputedValueNotRepresentable { field: "length" })
    ));

    let payload = [1, 2, 3];
    let mut short = [0xa5; 4];
    assert!(
        Packet::builder()
            .payload(&payload)
            .kind(1)
            .build_into(&mut short)
            .is_err()
    );
    assert_eq!(short, [0xa5; 4]);
}

#[test]
fn semantic_callback_receives_the_ordinary_field_by_reference() {
    let mut output = [0; 2];
    let (written, suffix) = SemanticCallback::builder()
        .value(7)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[8, 7]);
    assert!(suffix.is_empty());
}

#[test]
fn callbacks_accept_zero_arguments_and_empty_include_arguments() {
    let mut constant = [0; 1];
    let (written, suffix) = NoArgumentCallback::builder()
        .build_into(&mut constant)
        .unwrap();
    assert_eq!(written.as_bytes(), &[7]);
    assert!(suffix.is_empty());

    let mut empty = [0; 2];
    let (written, suffix) = EmptyIncludeCallback::builder()
        .value(9)
        .build_into(&mut empty)
        .unwrap();
    assert_eq!(written.as_bytes(), &[0, 9]);
    assert!(suffix.is_empty());
}

#[test]
fn callback_arguments_preserve_mixed_semantic_and_physical_order() {
    let mut output = [0; 5];
    let (written, suffix) = OrderedCallback::builder()
        .kind(2)
        .first(7)
        .second(8)
        .tail(9)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[213, 2, 7, 8, 9]);
    assert!(suffix.is_empty());
}

#[test]
fn duplicate_physical_selectors_are_a_set() {
    let mut output = [0; 2];
    let (written, suffix) = DuplicateSelection::builder()
        .value(7)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[1, 7]);
    assert!(suffix.is_empty());
}

#[test]
fn callback_usize_result_converts_to_computed_destination_type() {
    let mut output = [0; 2];
    let (written, suffix) = CallbackCount::builder()
        .value(7)
        .build_into(&mut output)
        .unwrap();

    assert_eq!(written.as_bytes(), &[1, 7]);
    assert!(suffix.is_empty());
}

#[test]
fn callback_conversion_failure_identifies_the_computed_destination() {
    assert!(matches!(
        OversizedCallback::builder().value(7).prepare(),
        Err(OversizedCallbackEncodeError::ComputedValueNotRepresentable { field: "count" })
    ));
}

#[test]
fn parsing_exposes_encoded_count_without_recomputing() {
    let parsed = RestPacket::view(&[5, 9, 8]).without_trailing().unwrap();
    assert_eq!(parsed.length(), 5);
    assert_eq!(parsed.payload(), [9, 8]);
}

#[test]
fn computed_builder_retains_nested_borrowed_inputs() {
    let payload = [1, 2];
    let data = [4, 5];
    let mut output = [0; 6];
    let (written, _) = NestedPacket::builder()
        .payload(&payload)
        .tail(NestedTail {
            kind: 3,
            data: &data,
        })
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[2, 1, 2, 3, 4, 5]);
}

#[test]
fn fixed_only_callback_builder_has_no_public_lifetime_and_prepares_bytes() {
    let builder: FixedCallbackBuilder = FixedCallback::builder().first(1).second(2);
    let plan: FixedCallbackPlan<'static> = builder.prepare().unwrap();
    assert_eq!(plan.encoded_len(), 3);

    let mut output = [0; 3];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[3, 1, 2]);
    assert_eq!(suffix, &mut []);
}

#[test]
fn fixed_only_exclude_self_builder_builds_an_empty_selection() {
    let builder: FixedExcludeSelfBuilder = FixedExcludeSelf::builder();
    let mut output = [0xff; 1];
    let (written, suffix) = builder.build_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[0]);
    assert_eq!(suffix, &mut []);
}

#[test]
fn callback_reads_selected_prepared_bytes_in_dependency_order() {
    let payload = [1, 2];
    let mut output = [0; 4];
    let (written, _) = Checksummed::builder()
        .payload(&payload)
        .build_into(&mut output)
        .unwrap();

    assert_eq!(written.as_bytes(), &[5, 2, 1, 2]);
    let view = Checksummed::view(written.as_bytes())
        .without_trailing()
        .unwrap();
    assert_eq!(view.checksum(), 5);
    assert_eq!(view.length(), 2);
    assert_eq!(view.payload(), payload);
}

#[test]
fn include_read_sets_order_computations_without_using_declaration_order() {
    let payload = [1, 2];
    let mut output = [0; 4];
    let (written, _) = IncludedDependency::builder()
        .payload(&payload)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[6, 3, 1, 2]);
}

#[test]
fn callback_can_pull_a_nested_prepared_field_selection() {
    let data = [8, 9];
    let mut output = [0; 4];
    let (written, _) = NestedSelection::builder()
        .tail(NestedTail {
            kind: 7,
            data: &data,
        })
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[7, 7, 8, 9]);
}

#[test]
fn exclude_self_source_contains_generated_physical_gaps() {
    let mut output = [0xff; 6];
    let (written, _) = PositionedChecksum::builder()
        .marker(7)
        .tail(8)
        .payload(&[])
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[5, 0, 0, 0, 7, 8]);
}

#[test]
fn computed_derivation_and_exact_source_validation_share_the_byte_source_abi() {
    let payload = [1, 2, 3];
    let mut output = [0; 4];
    let (written, _) = ValidatedChecksum::builder()
        .payload(&payload)
        .build_into(&mut output)
        .unwrap();
    assert_eq!(written.as_bytes(), &[6, 1, 2, 3]);
    assert!(
        ValidatedChecksum::view(written.as_bytes())
            .without_trailing()
            .is_ok()
    );

    assert!(matches!(
        ValidatedChecksum::view(&[7, 1, 2, 3]).without_trailing(),
        Err(ChecksumError::Mismatch)
    ));
    let unchecked = ValidatedChecksum::view(&[7, 1, 2, 3])
        .unchecked()
        .without_trailing()
        .unwrap();
    assert_eq!(unchecked.checksum(), 7);
}

#[test]
fn table_bound_fixed_builder_computes_selected_nested_bytes_and_drops_table() {
    let plan: CallbackEnvelopePlan<'static> = {
        let table = CallbackTable { tag: 0x31 };
        let builder: CallbackEnvelopeBuilder = CallbackEnvelope::builder()
            .selected(CallbackOperation::Ping(CallbackBody { value: 4 }));
        builder.table(&table).prepare().unwrap()
    };
    assert_eq!(plan.encoded_len(), 3);

    let mut output = [0; 3];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[0x35, 0x31, 4]);
    assert_eq!(suffix, &mut []);

    let table = CallbackTable { tag: 0x31 };
    let view = CallbackEnvelope::view(written.as_bytes())
        .table(&table)
        .without_trailing()
        .unwrap();
    assert_eq!(view.checksum(), 0x35);
    assert_eq!(view.selected().ping().unwrap().value(), 4);

    assert!(matches!(
        CallbackEnvelope::builder().table(&table).prepare(),
        Err(CallbackEnvelopeEncodeError::MissingField { field: "selected" })
    ));
}

#[test]
fn table_bound_borrowed_builder_computes_length_before_geometry_and_is_atomic() {
    let payload = [7, 8];
    let mut short = [0xa5; 4];
    {
        let table = CallbackTable { tag: 0x31 };
        assert!(
            CallbackPayload::builder()
                .payload(&payload)
                .selected(CallbackOperation::Ping(CallbackBody { value: 4 }))
                .table(&table)
                .build_into(&mut short)
                .is_err()
        );
    }
    assert_eq!(short, [0xa5; 4]);

    let plan = {
        let table = CallbackTable { tag: 0x31 };
        CallbackPayload::builder()
            .payload(&payload)
            .selected(CallbackOperation::Ping(CallbackBody { value: 4 }))
            .table(&table)
            .prepare()
            .unwrap()
    };
    assert_eq!(plan.encoded_len(), 5);

    let mut output = [0; 5];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[2, 7, 8, 0x31, 4]);
    assert_eq!(suffix, &mut []);

    let table = CallbackTable { tag: 0x31 };
    let view = CallbackPayload::view(written.as_bytes())
        .table(&table)
        .without_trailing()
        .unwrap();
    assert_eq!(view.length(), 2);
    assert_eq!(view.payload(), payload);
    assert_eq!(view.selected().ping().unwrap().value(), 4);
}
