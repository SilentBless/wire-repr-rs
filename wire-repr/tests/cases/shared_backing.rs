use bytes::{Bytes, BytesMut};
use wire_repr::{
    ByteSegment, ByteSink, ByteSource, ByteSourceCursor, FixedViewIterator, OutputTooShortError,
    PreparedLayout, ViewCursor, ViewCursorError, ViewRequest, Wire, WireView, Written,
};

#[derive(Wire)]
#[wire(bitfield = u16, be, reserved = zero)]
struct OwnedFlags {
    #[wire(bit = 0)]
    enabled: bool,
    #[wire(bits = 1..=3)]
    mode: u8,
}

#[derive(Wire)]
struct OwnedPacket<'wire> {
    length: u8,
    #[wire(bytes = length)]
    payload: &'wire [u8],
}

#[derive(Wire)]
struct OwnedBody {
    value: u8,
}

#[derive(Wire)]
#[wire(tag = U8, unknown = reject)]
#[repr(u8)]
enum OwnedOperation {
    Data(OwnedBody) = 1,
    Halt = 2,
}

#[derive(Clone, Copy, PartialEq)]
enum OwnedSelector {
    Data,
    Halt,
}

struct OwnedTable;

impl OwnedTable {
    fn decode(&self, value: u8) -> Result<Option<OwnedSelector>, core::convert::Infallible> {
        Ok(match value {
            7 => Some(OwnedSelector::Data),
            8 => Some(OwnedSelector::Halt),
            _ => None,
        })
    }

    fn encode(&self, selector: OwnedSelector) -> Result<Option<u8>, core::convert::Infallible> {
        Ok(Some(match selector {
            OwnedSelector::Data => 7,
            OwnedSelector::Halt => 8,
        }))
    }
}

#[derive(Wire)]
#[wire(tag = U8, table = OwnedTable, table_error = core::convert::Infallible, unknown = reject)]
enum OwnedMappedOperation {
    #[wire(table = OwnedSelector::Data)]
    Data(OwnedBody),
    #[wire(table = OwnedSelector::Halt)]
    Halt,
}

#[derive(Wire)]
#[wire(table = OwnedTable)]
struct OwnedMappedPacket {
    lead: u8,
    #[wire(table)]
    operation: OwnedMappedOperation,
}

#[derive(Wire)]
struct OwnedOuter {
    lead: u8,
    body: OwnedBody,
    tail: u8,
}

#[derive(Wire)]
#[wire(tag = [u8; 4], unknown = preserve)]
enum OwnedOpenOperation {
    #[wire(tag = b"HALT")]
    Halt,
    #[wire(unknown)]
    Other([u8; 4]),
}

#[derive(Debug)]
enum OwnedValidationError {
    Decode(OwnedValidatedDecodeError),
    Invalid,
}

impl core::fmt::Display for OwnedValidationError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl core::error::Error for OwnedValidationError {}

impl From<OwnedValidatedDecodeError> for OwnedValidationError {
    fn from(error: OwnedValidatedDecodeError) -> Self {
        Self::Decode(error)
    }
}

fn nonzero(value: u8) -> Result<(), OwnedValidationError> {
    if value == 0 {
        Err(OwnedValidationError::Invalid)
    } else {
        Ok(())
    }
}

#[derive(Wire)]
#[wire(error = OwnedValidationError)]
struct OwnedValidated {
    #[wire(validate = nonzero)]
    value: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ByteView {
    bytes: Bytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DecodeError {
    Empty,
    Rejected,
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl core::error::Error for DecodeError {}

impl<'wire> WireView<'wire> for ByteView {
    type DecodeError = DecodeError;

    fn parse_view(mut input: Bytes) -> Result<(Self, Bytes), Self::DecodeError> {
        match input.first() {
            None => Err(DecodeError::Empty),
            Some(0xff) => Err(DecodeError::Rejected),
            Some(_) => Ok((
                Self {
                    bytes: input.split_to(1),
                },
                input,
            )),
        }
    }

    fn trailing_bytes_error(_: usize, _: usize) -> Self::DecodeError {
        DecodeError::Rejected
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

struct SegmentedPlan;

impl ByteSource for SegmentedPlan {
    fn byte_len(&self) -> usize {
        4
    }

    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        sink.write(&[0x10, 0x20]);
        sink.fill(0xa5, 2);
    }
}

impl ByteSourceCursor for SegmentedPlan {
    type Segments<'source> = core::iter::Once<ByteSegment<'source>>;

    fn segments(&self) -> Self::Segments<'_> {
        core::iter::once(ByteSegment::Bytes(&[0x10, 0x20, 0xa5, 0xa5]))
    }

    type Bytes<'source>
        = wire_repr::ByteBytes<'source, Self::Segments<'source>>
    where
        Self: 'source;

    fn bytes(&self) -> Self::Bytes<'_> {
        wire_repr::ByteBytes::new(self.segments())
    }
}

impl PreparedLayout for SegmentedPlan {
    type Written<'output> = Written<'output>;

    fn commit_into<'output>(
        self,
        output: &'output mut BytesMut,
    ) -> Result<Self::Written<'output>, OutputTooShortError> {
        Ok(Written::new(self.append_into_bytes_mut(output)?))
    }
}

#[test]
fn owned_request_remainder_and_view_survive_input_handoff() {
    fn assert_clone<T: Clone>() {}
    assert_clone::<ByteView>();

    let (view, remainder) = ViewRequest::<ByteView>::new(Bytes::from_static(&[1, 2, 3]))
        .with_remainder()
        .unwrap();
    assert_eq!(view.as_bytes(), &[1]);
    assert_eq!(&remainder[..], &[2, 3]);

    let transferred = view.clone();
    drop(view);
    assert_eq!(transferred.as_bytes(), &[1]);
    assert_eq!(&remainder[..], &[2, 3]);
}

#[test]
fn fixed_view_iterator_transfers_owned_item_ranges() {
    let mut views =
        FixedViewIterator::new(Bytes::from_static(&[4, 5]), 1, |bytes| ByteView { bytes }).unwrap();
    assert_eq!(views.len(), 2);
    assert_eq!(views.next().unwrap().as_bytes(), &[4]);
    assert_eq!(views.next().unwrap().as_bytes(), &[5]);
    assert!(views.next().is_none());
}

#[test]
fn generated_fixed_sequence_owns_each_item_range() {
    let mut views = OwnedBody::views(Bytes::from_static(&[4, 5])).unwrap();
    let first = views.next().unwrap();
    let cloned = first.clone();
    assert_eq!(first.as_bytes().as_ptr(), cloned.as_bytes().as_ptr());
    drop(first);

    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(cloned).unwrap();
    assert_eq!(receiver.recv().unwrap().value(), 4);
    assert_eq!(views.next().unwrap().value(), 5);
    assert!(views.next().is_none());
}

#[test]
fn generated_owned_validation_covers_direct_and_sequence_views() {
    match OwnedValidated::view(Bytes::new()).without_trailing() {
        Err(OwnedValidationError::Decode(error)) => {
            assert!(!format!("{error:?}").is_empty());
        }
        result => panic!("unexpected structural validation result: {result:?}"),
    }
    assert!(matches!(
        OwnedValidated::view(Bytes::from_static(&[0])).without_trailing(),
        Err(OwnedValidationError::Invalid)
    ));
    assert!(matches!(
        OwnedValidated::views(Bytes::from_static(&[1, 0])),
        Err(wire_repr::FixedValidatedViewSequenceError::Item(
            OwnedValidationError::Invalid
        ))
    ));

    let mut unchecked = OwnedValidated::unchecked_views(Bytes::from_static(&[1, 0])).unwrap();
    assert_eq!(unchecked.next().unwrap().value(), 1);
    assert_eq!(unchecked.next().unwrap().value(), 0);
}

#[test]
fn owned_cursor_advances_only_after_successful_parse() {
    let mut cursor = ViewCursor::<ByteView>::new(Bytes::from_static(&[1, 0xff, 2]));
    assert_eq!(cursor.next().unwrap().unwrap().as_bytes(), &[1]);
    assert_eq!(cursor.remaining(), &[0xff, 2]);
    assert_eq!(
        cursor.next(),
        Err(ViewCursorError::Item(DecodeError::Rejected))
    );
    assert_eq!(cursor.remaining(), &[0xff, 2]);
}

#[test]
fn commit_appends_without_moving_or_growing_preallocated_output() {
    let mut output = BytesMut::with_capacity(12);
    output.extend_from_slice(&[0xca, 0xfe]);
    let pointer = output.as_ptr();
    let capacity = output.capacity();

    {
        let written = SegmentedPlan.commit_into(&mut output).unwrap();
        assert_eq!(written.as_bytes(), &[0x10, 0x20, 0xa5, 0xa5]);
    }

    assert_eq!(&output[..], &[0xca, 0xfe, 0x10, 0x20, 0xa5, 0xa5]);
    assert_eq!(output.as_ptr(), pointer);
    assert_eq!(output.capacity(), capacity);
}

#[test]
fn short_output_is_unchanged_before_commit_mutation() {
    let mut output = BytesMut::with_capacity(5);
    output.extend_from_slice(&[0xca, 0xfe]);
    let bytes = output.to_vec();
    let pointer = output.as_ptr();
    let capacity = output.capacity();

    assert!(matches!(
        SegmentedPlan.commit_into(&mut output),
        Err(OutputTooShortError {
            required: 4,
            available: 3,
        })
    ));
    assert_eq!(&output[..], bytes);
    assert_eq!(output.as_ptr(), pointer);
    assert_eq!(output.capacity(), capacity);
}

#[test]
fn bounded_bytes_mut_sink_rejects_broken_source_without_publishing_partial_output() {
    struct BrokenSource;

    impl ByteSource for BrokenSource {
        fn byte_len(&self) -> usize {
            1
        }

        fn emit_to<S: ByteSink>(&self, sink: &mut S) {
            sink.write(&[1, 2]);
        }
    }

    let mut output = BytesMut::with_capacity(3);
    output.extend_from_slice(&[0xca]);
    let bytes = output.to_vec();
    let pointer = output.as_ptr();
    let capacity = output.capacity();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            BrokenSource.append_into_bytes_mut(&mut output).unwrap();
        }))
        .is_err()
    );
    assert_eq!(&output[..], bytes);
    assert_eq!(output.as_ptr(), pointer);
    assert_eq!(output.capacity(), capacity);
}

#[test]
fn bounded_bytes_mut_sink_does_not_publish_under_emission() {
    struct BrokenSource;

    impl ByteSource for BrokenSource {
        fn byte_len(&self) -> usize {
            2
        }

        fn emit_to<S: ByteSink>(&self, sink: &mut S) {
            sink.write(&[1]);
        }
    }

    let mut output = BytesMut::with_capacity(3);
    output.extend_from_slice(&[0xca]);
    let bytes = output.to_vec();
    let pointer = output.as_ptr();
    let capacity = output.capacity();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            BrokenSource.append_into_bytes_mut(&mut output).unwrap();
        }))
        .is_err()
    );
    assert_eq!(&output[..], bytes);
    assert_eq!(output.as_ptr(), pointer);
    assert_eq!(output.capacity(), capacity);
}

#[test]
fn derived_bitfield_uses_owned_view_and_preallocated_output() {
    let view = OwnedFlags::view(Bytes::from_static(&[0, 5]))
        .without_trailing()
        .unwrap();
    assert!(view.enabled());
    assert_eq!(view.mode(), 2);

    let mut output = BytesMut::with_capacity(2);
    OwnedFlags {
        enabled: true,
        mode: 2,
    }
    .build_into(&mut output)
    .unwrap();
    assert_eq!(&output[..], &[0, 5]);
}

#[test]
fn derived_dynamic_struct_owns_input_and_borrows_builder_payload() {
    let view = OwnedPacket::view(Bytes::from_static(&[3, 7, 8, 9]))
        .without_trailing()
        .unwrap();
    assert_eq!(view.length(), 3);
    assert_eq!(view.payload(), &[7, 8, 9]);

    let payload = [7, 8, 9];
    let semantic = OwnedPacket {
        length: u8::MAX,
        payload: &payload,
    };
    assert_eq!(semantic.length, u8::MAX);
    let plan = OwnedPacket::builder().payload(&payload).prepare().unwrap();
    let mut output = BytesMut::with_capacity(plan.encoded_len());
    plan.commit_into(&mut output).unwrap();
    assert_eq!(&output[..], &[3, 7, 8, 9]);
}

#[test]
fn derived_enum_owns_tag_and_nested_body() {
    let (view, remainder) = OwnedOperation::view(Bytes::from_static(&[1, 9, 0xaa]))
        .with_remainder()
        .unwrap();
    assert_eq!(view.data().unwrap().value(), 9);
    assert_eq!(&remainder[..], &[0xaa]);

    let mut output = BytesMut::with_capacity(2);
    OwnedOperation::Data(OwnedBody { value: 9 })
        .build_into(&mut output)
        .unwrap();
    assert_eq!(&output[..], &[1, 9]);
    let mut halt = BytesMut::with_capacity(1);
    OwnedOperation::Halt.build_into(&mut halt).unwrap();
    assert_eq!(&halt[..], &[2]);
}

#[test]
fn owned_enum_preserves_unknown_fixed_byte_tags() {
    let raw = Bytes::from_static(&[0xff, 0, b'X', 0x80]);
    let view = OwnedOpenOperation::view(raw.clone())
        .without_trailing()
        .unwrap();
    assert_eq!(view.other(), Some([0xff, 0, b'X', 0x80]));
    assert_eq!(view.as_bytes(), raw.as_ref());

    let mut output = BytesMut::with_capacity(4);
    OwnedOpenOperation::Other([0xff, 0, b'X', 0x80])
        .build_into(&mut output)
        .unwrap();
    assert_eq!(output.as_ref(), raw.as_ref());

    let halt = OwnedOpenOperation::view(Bytes::from_static(b"HALT"))
        .without_trailing()
        .unwrap();
    assert!(halt.is_halt());

    let mut encoded_halt = BytesMut::with_capacity(4);
    OwnedOpenOperation::Halt
        .build_into(&mut encoded_halt)
        .unwrap();
    assert_eq!(encoded_halt.as_ref(), b"HALT");
}

#[test]
fn nested_owned_view_survives_parent() {
    let parent = OwnedOuter::view(Bytes::from_static(&[1, 9, 2]))
        .without_trailing()
        .unwrap();
    let child = parent.body();
    drop(parent);
    assert_eq!(child.value(), 9);
    assert_eq!(child.as_bytes(), &[9]);
}

#[test]
fn operation_bound_struct_and_enum_do_not_retain_the_table() {
    let table = OwnedTable;
    let view = OwnedMappedPacket::view(Bytes::from_static(&[4, 7, 9]))
        .table(&table)
        .without_trailing()
        .unwrap();
    assert_eq!(view.lead(), 4);
    assert_eq!(view.operation().data().unwrap().value(), 9);

    let mut output = BytesMut::with_capacity(3);
    OwnedMappedPacket {
        lead: 4,
        operation: OwnedMappedOperation::Data(OwnedBody { value: 9 }),
    }
    .table(&table)
    .build_into(&mut output)
    .unwrap();
    assert_eq!(&output[..], &[4, 7, 9]);

    let mut halt = BytesMut::with_capacity(1);
    OwnedMappedOperation::Halt
        .table(&table)
        .build_into(&mut halt)
        .unwrap();
    assert_eq!(&halt[..], &[8]);
}
