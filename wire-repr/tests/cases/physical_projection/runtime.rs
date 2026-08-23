use core::mem::size_of;
use core::ops::{BitOr, Range};

use wire_repr::{
    ByteSegment, ByteSelection, ByteSink, ByteSource, ByteSourceCursor, FieldSelection, FieldUnion,
};

struct Segmented;

impl ByteSource for Segmented {
    fn byte_len(&self) -> usize {
        12
    }

    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        sink.write(&[0, 1, 2]);
        sink.fill(0xa5, 4);
        sink.write(&[7, 8]);
        sink.fill(0xb6, 2);
        sink.write(&[11]);
    }
}

impl ByteSourceCursor for Segmented {
    type Segments<'source> = core::array::IntoIter<ByteSegment<'source>, 5>;

    fn segments(&self) -> Self::Segments<'_> {
        [
            ByteSegment::Bytes(&[0, 1, 2]),
            ByteSegment::Rest { byte: 0xa5, len: 4 },
            ByteSegment::Bytes(&[7, 8]),
            ByteSegment::Rest { byte: 0xb6, len: 2 },
            ByteSegment::Bytes(&[11]),
        ]
        .into_iter()
    }

    type Bytes<'source>
        = wire_repr::ByteBytes<'source, Self::Segments<'source>>
    where
        Self: 'source;

    fn bytes(&self) -> Self::Bytes<'_> {
        wire_repr::ByteBytes::new(self.segments())
    }
}

struct OverEmitting;

impl ByteSource for OverEmitting {
    fn byte_len(&self) -> usize {
        12
    }

    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        Segmented.emit_to(sink);
        sink.write(&[12]);
    }
}

// A handwritten stand-in for the ZST proxy a future derive will generate.
#[derive(Clone, Copy)]
struct Fields {
    header: Header,
    payload: Payload,
    signature: Signature,
    empty: Empty,
    chaotic: Chaotic,
}

#[derive(Clone, Copy)]
struct Header {
    checksum: Checksum,
}

#[derive(Clone, Copy)]
struct Payload;
#[derive(Clone, Copy)]
struct Checksum;
#[derive(Clone, Copy)]
struct Signature;
#[derive(Clone, Copy)]
struct Empty;
#[derive(Clone, Copy)]
struct Chaotic;
#[derive(Clone, Copy)]
struct BadStart;
#[derive(Clone, Copy)]
struct BadEnd;

macro_rules! marker_union {
    ($($marker:ty),+ $(,)?) => {
        $(
            impl<R> BitOr<R> for $marker {
                type Output = FieldUnion<Self, R>;

                fn bitor(self, right: R) -> Self::Output {
                    FieldUnion::new(self, right)
                }
            }
        )+
    };
}

marker_union!(
    Header, Payload, Checksum, Signature, Empty, Chaotic, BadStart, BadEnd
);

macro_rules! ranges {
    ($marker:ty => $($range:expr),* $(,)?) => {
        impl<T: ?Sized> FieldSelection<T> for $marker {
            fn visit_ranges<V>(&self, _: &T, visitor: &mut V)
            where
                V: FnMut(Range<usize>),
            {
                $(visitor($range);)*
            }
        }
    };
}

ranges!(Header => 0..2);
ranges!(Checksum => 2..3);
ranges!(Payload => 4..7);
ranges!(Signature => 7..9);
ranges!(Empty => 3..3);
ranges!(Chaotic => 8..10, 2..7, 2..7, 3..4, 7..8, 0..0);
ranges!(BadStart => Range { start: 5, end: 4 });
ranges!(BadEnd => 0..13);

fn root<T: ByteSource>(source: &T) -> ByteSelection<'_, T, Fields> {
    ByteSelection::new(
        source,
        Fields {
            header: Header { checksum: Checksum },
            payload: Payload,
            signature: Signature,
            empty: Empty,
            chaotic: Chaotic,
        },
    )
}

fn bytes(source: impl ByteSource) -> std::vec::Vec<u8> {
    let mut output = std::vec![0; source.byte_len()];
    source.write_into(&mut output);
    output
}

#[test]
fn overlapping_reversed_duplicate_contained_and_adjacent_ranges_are_a_set() {
    let source = Segmented;
    let selected = root(&source).include(|f| f.chaotic);
    assert_eq!(selected.byte_len(), 8);
    assert_eq!(bytes(selected), [2, 0xa5, 0xa5, 0xa5, 0xa5, 7, 8, 0xb6]);
    let excluded = root(&source).exclude(|f| f.chaotic);
    assert_eq!(excluded.byte_len(), 4);
    assert_eq!(bytes(excluded), [0, 1, 0xb6, 11]);
}

#[test]
fn selected_sources_support_the_same_pull_cursor_as_complete_sources() {
    let source = Segmented;
    let selected = root(&source).include(|f| f.chaotic);
    assert_eq!(
        selected.bytes().collect::<Vec<_>>(),
        [2, 0xa5, 0xa5, 0xa5, 0xa5, 7, 8, 0xb6]
    );
    assert_eq!(
        selected.segments().collect::<Vec<_>>(),
        [
            ByteSegment::Bytes(&[2]),
            ByteSegment::Rest { byte: 0xa5, len: 4 },
            ByteSegment::Bytes(&[7, 8]),
            ByteSegment::Rest { byte: 0xb6, len: 1 },
        ]
    );

    let excluded = root(&source).exclude(|f| f.payload | f.signature);
    assert_eq!(
        excluded.bytes().collect::<Vec<_>>(),
        [0, 1, 2, 0xa5, 0xb6, 0xb6, 11]
    );
}

#[test]
fn gaps_are_not_fields_but_survive_exclusion() {
    let source = Segmented;
    assert_eq!(
        bytes(root(&source).include(|f| f.payload)),
        [0xa5, 0xa5, 0xa5]
    );
    assert_eq!(
        bytes(root(&source).exclude(|f| f.payload)),
        [0, 1, 2, 0xa5, 7, 8, 0xb6, 0xb6, 11]
    );

    assert_eq!(root(&source).include(|f| f.empty).byte_len(), 0);
    assert_eq!(bytes(root(&source).include(|f| f.empty)), []);
    assert_eq!(
        bytes(root(&source).exclude(|f| f.empty)),
        [0, 1, 2, 0xa5, 0xa5, 0xa5, 0xa5, 7, 8, 0xb6, 0xb6, 11]
    );
}

struct RuntimeGeometry {
    header_len: usize,
    payload: [u8; 4],
    payload_len: usize,
    trailer_len: usize,
}

impl ByteSource for RuntimeGeometry {
    fn byte_len(&self) -> usize {
        self.header_len + self.payload_len + self.trailer_len
    }

    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        sink.fill(0xaa, self.header_len);
        sink.write(&self.payload[..self.payload_len]);
        sink.fill(0xbb, self.trailer_len);
    }
}

#[derive(Clone, Copy)]
struct RuntimeFields {
    payload: RuntimePayload,
}

#[derive(Clone, Copy)]
struct RuntimePayload;

impl FieldSelection<RuntimeGeometry> for RuntimePayload {
    fn visit_ranges<V>(&self, target: &RuntimeGeometry, visitor: &mut V)
    where
        V: FnMut(Range<usize>),
    {
        visitor(target.header_len..target.header_len + target.payload_len);
    }
}

fn runtime_root(source: &RuntimeGeometry) -> ByteSelection<'_, RuntimeGeometry, RuntimeFields> {
    ByteSelection::new(
        source,
        RuntimeFields {
            payload: RuntimePayload,
        },
    )
}

#[test]
fn zst_marker_uses_each_target_runtime_geometry() {
    let short = RuntimeGeometry {
        header_len: 1,
        payload: [10, 11, 0, 0],
        payload_len: 2,
        trailer_len: 1,
    };
    let long = RuntimeGeometry {
        header_len: 3,
        payload: [20, 21, 22, 23],
        payload_len: 4,
        trailer_len: 2,
    };

    assert_eq!(size_of::<RuntimePayload>(), 0);
    assert_eq!(
        bytes(runtime_root(&short).include(|fields| fields.payload)),
        [10, 11]
    );
    assert_eq!(
        bytes(runtime_root(&long).include(|fields| fields.payload)),
        [20, 21, 22, 23]
    );
    assert_eq!(
        bytes(runtime_root(&long).exclude(|fields| fields.payload)),
        [0xaa, 0xaa, 0xaa, 0xbb, 0xbb]
    );
}

struct FillRecordingSink {
    output: [u8; 12],
    written: usize,
    fills: [(u8, usize); 4],
    fill_count: usize,
}

impl ByteSink for FillRecordingSink {
    fn write(&mut self, bytes: &[u8]) {
        let end = self.written + bytes.len();
        self.output[self.written..end].copy_from_slice(bytes);
        self.written = end;
    }

    fn fill(&mut self, byte: u8, len: usize) {
        self.fills[self.fill_count] = (byte, len);
        self.fill_count += 1;
        let end = self.written + len;
        self.output[self.written..end].fill(byte);
        self.written = end;
    }
}

#[test]
fn filtering_splits_chunks_and_preserves_selected_fill_runs() {
    let source = Segmented;
    let selected = root(&source).include(|f| f.chaotic);
    let mut sink = FillRecordingSink {
        output: [0; 12],
        written: 0,
        fills: [(0, 0); 4],
        fill_count: 0,
    };
    selected.emit_to(&mut sink);

    assert_eq!(sink.written, selected.byte_len());
    assert_eq!(
        &sink.output[..sink.written],
        [2, 0xa5, 0xa5, 0xa5, 0xa5, 7, 8, 0xb6]
    );
    assert_eq!(
        &sink.fills[..sink.fill_count],
        [(0xa5, 1), (0xa5, 3), (0xb6, 1)]
    );
}

#[test]
fn invalid_ranges_panic_during_selection_construction() {
    let source = Segmented;
    assert!(std::panic::catch_unwind(|| root(&source).include(|_| BadStart)).is_err());
    assert!(std::panic::catch_unwind(|| root(&source).exclude(|_| BadEnd)).is_err());
}

#[test]
fn selection_detects_wrapped_sources_that_under_or_over_emit() {
    struct ActuallyUnder;
    impl ByteSource for ActuallyUnder {
        fn byte_len(&self) -> usize {
            12
        }
        fn emit_to<S: ByteSink>(&self, sink: &mut S) {
            sink.write(&[0, 1, 2]);
            sink.fill(0xa5, 4);
            sink.write(&[7, 8]);
            sink.fill(0xb6, 2);
        }
    }

    assert!(std::panic::catch_unwind(|| bytes(root(&ActuallyUnder).exclude(|f| f.empty))).is_err());
    assert!(std::panic::catch_unwind(|| bytes(root(&OverEmitting).exclude(|f| f.empty))).is_err());
}
