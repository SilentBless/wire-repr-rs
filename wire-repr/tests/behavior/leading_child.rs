#![allow(dead_code)]

use core::convert::Infallible;
use wire_repr::{NeedMore, WireBuilder, WireView, WriteError, select, wire};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(WireView, WireBuilder)]
struct Segment {
    length: u8,
    #[wire(bytes = length)]
    payload: wire::Bytes,
}

#[derive(WireView, WireBuilder)]
struct Packet<T> {
    prefix: u8,
    child: T,
    #[wire(be)]
    suffix: u16,
}

#[derive(WireView, WireBuilder)]
struct Paired {
    prefix: u8,
    first: Segment,
    #[wire(be, pad_before = 1)]
    middle: u16,
    second: Segment,
    suffix: u8,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum Envelope<T> {
    #[wire(value = 0x45)]
    Record(Packet<T>),
}

#[derive(WireView, WireBuilder)]
struct EnvelopePacket {
    prefix: u8,
    envelope: Envelope<Segment>,
    suffix: u8,
}

#[derive(WireBuilder)]
struct Forwarded<T> {
    count: u8,
    #[wire(counted_by = count)]
    items: wire::Array<T>,
    suffix: u8,
}

#[test]
fn derived_leading_child_before_a_scalar_reads_and_writes() -> TestResult {
    let expected = [0x31, 2, 0x52, 0x53, 0x64, 0x65];
    let view = Packet::<Segment>::view(&expected[..])?;
    assert_eq!(view.prefix(), 0x31);
    assert_eq!(view.child().length(), 2);
    assert_eq!(view.child().payload(), [0x52, 0x53]);
    assert_eq!(view.child().as_bytes(), &expected[1..4]);
    assert_eq!(view.suffix(), 0x6465);
    assert!(matches!(
        Packet::<Segment>::view([0x31, 2, 0x52, 0x53, 0x64, 0x65, 0x76]),
        Err(PacketViewError::Trailing(wire_repr::TrailingBytes {
            offset: 6,
            trailing: 1,
        }))
    ));

    let mut output = [0xcc; 8];
    let written = Packet::<Segment>::builder(&mut output[..])
        .prefix(0x31)?
        .child(|child| child.payload(&[0x52, 0x53][..]))?
        .suffix(0x6465)?
        .finish()?;
    assert_eq!(written.as_bytes(), expected);
    assert_eq!(&output[6..], &[0xcc, 0xcc]);
    Ok(())
}

#[test]
fn two_children_delimit_empty_and_nonempty_payloads_around_fixed_fields() -> TestResult {
    for (first, second, expected) in [
        (&[][..], &[][..], &[0x31, 0, 0, 0x64, 0x65, 0, 0x76][..]),
        (
            &[][..],
            &[0x52, 0x53][..],
            &[0x31, 0, 0, 0x64, 0x65, 2, 0x52, 0x53, 0x76][..],
        ),
        (
            &[0x41][..],
            &[][..],
            &[0x31, 1, 0x41, 0, 0x64, 0x65, 0, 0x76][..],
        ),
        (
            &[0x41][..],
            &[0x52, 0x53][..],
            &[0x31, 1, 0x41, 0, 0x64, 0x65, 2, 0x52, 0x53, 0x76][..],
        ),
    ] {
        let mut output = [0xcc; 10];
        let written = Paired::builder(&mut output[..])
            .prefix(0x31)?
            .first(|child| child.payload(first))?
            .middle(0x6465)?
            .second(|child| child.payload(second))?
            .suffix(0x76)?
            .finish()?;
        assert_eq!(written.as_bytes(), expected);
        let view = Paired::view(written.as_bytes())?;
        assert_eq!(view.first().payload(), first);
        assert_eq!(view.middle(), 0x6465);
        assert_eq!(view.second().payload(), second);
        assert_eq!(view.suffix(), 0x76);
    }
    Ok(())
}

#[test]
fn enum_body_and_detached_parent_preserve_the_nested_cursor() -> TestResult {
    let expected = [0x21, 0x45, 0x31, 2, 0x52, 0x53, 0x64, 0x65, 0x76];
    let mut output = [0xcc; 9];
    let written = EnvelopePacket::builder(&mut output[..])
        .prefix(0x21)?
        .envelope(|envelope| {
            envelope.record(|packet| {
                packet
                    .prefix(0x31)
                    .child(|child| child.payload(&[0x52, 0x53][..]))
                    .suffix(0x6465)
            })
        })?
        .suffix(0x76)?
        .finish()?;
    assert_eq!(written.as_bytes(), expected);
    let view = EnvelopePacket::view(written.as_bytes())?;
    let envelope = view.envelope();
    let EnvelopeVariant::Record(packet) = envelope.variant();
    assert_eq!(packet.child().payload(), [0x52, 0x53]);
    assert_eq!(packet.suffix(), 0x6465);
    assert_eq!(view.suffix(), 0x76);
    Ok(())
}

#[test]
fn retained_backing_and_selections_use_exact_child_ranges_and_copy_source_padding() -> TestResult {
    let input = vec![0x31, 1, 0x41, 0xad, 0x64, 0x65, 2, 0x52, 0x53, 0x76];
    let pointer = input.as_ptr();
    let view = Paired::view(input)?;
    assert_eq!(view.as_bytes().as_ptr(), pointer);
    assert_eq!(
        view.first().as_bytes().as_ptr(),
        view.as_bytes()[1..].as_ptr()
    );
    assert_eq!(view.second().as_bytes(), &view.as_bytes()[6..9]);
    assert_eq!(
        view.second().payload().as_ptr(),
        view.as_bytes()[7..].as_ptr()
    );
    assert_eq!(view.middle(), 0x6465);

    let whole = select(&view).include(|fields| fields.second | fields.first);
    assert_eq!(
        whole.chunks().collect::<Vec<_>>(),
        [&view.as_bytes()[1..3], &view.as_bytes()[6..9]],
    );
    let nested = select(&view).include(|fields| {
        fields.second.fields(|child| child.payload) | fields.first.fields(|child| child.payload)
    });
    assert_eq!(
        nested.chunks().collect::<Vec<_>>(),
        [&view.as_bytes()[2..3], &view.as_bytes()[7..9]],
    );
    let overlap =
        select(&view).include(|fields| fields.first | fields.first.fields(|child| child.payload));
    assert_eq!(
        overlap.chunks().collect::<Vec<_>>(),
        [&view.as_bytes()[1..3]]
    );

    let mut output = [0xcc; 12];
    let written = Forwarded::<Paired>::builder(&mut output[..])
        .items(|items| items.item_view(view))?
        .suffix(0x87)?
        .finish()?;
    assert_eq!(
        written.as_bytes(),
        [
            1, 0x31, 1, 0x41, 0xad, 0x64, 0x65, 2, 0x52, 0x53, 0x76, 0x87
        ],
    );
    Ok(())
}

#[test]
fn parent_sequences_and_cursors_stop_at_each_leading_boundary() -> TestResult {
    let input = [0x31, 1, 0x41, 0x64, 0x65, 0x32, 0, 0x74, 0x75, 0x86];
    let (first, mut cursor) = Packet::<Segment>::cursor(&input)?;
    let second = Packet::<Segment>::next(&mut cursor)?;
    assert_eq!(first.child().payload(), [0x41]);
    assert_eq!(first.suffix(), 0x6465);
    assert_eq!(second.child().payload(), []);
    assert_eq!(second.suffix(), 0x7475);
    assert_eq!(second.as_bytes(), &input[5..9]);
    assert_eq!(cursor.position(), 9);
    assert_eq!(cursor.remaining(), &[0x86]);

    let mut views = Packet::<Segment>::views(&input[..9])?;
    assert_eq!(views.next()?.expect("first packet").as_bytes(), &input[..5]);
    assert_eq!(
        views.next()?.expect("second packet").as_bytes(),
        &input[5..9]
    );
    assert!(views.next()?.is_none());
    Ok(())
}

#[test]
fn child_prefix_body_and_parent_suffix_shortages_keep_absolute_field_context() {
    for (input, expected) in [
        (
            &[][..],
            PacketViewError::Prefix(NeedMore {
                offset: 17,
                additional_at_least: 1,
            }),
        ),
        (
            &[0x31][..],
            PacketViewError::Child(SegmentViewError::Length(NeedMore {
                offset: 18,
                additional_at_least: 1,
            })),
        ),
        (
            &[0x31, 2][..],
            PacketViewError::Child(SegmentViewError::Payload(NeedMore {
                offset: 19,
                additional_at_least: 2,
            })),
        ),
        (
            &[0x31, 2, 0x52][..],
            PacketViewError::Child(SegmentViewError::Payload(NeedMore {
                offset: 20,
                additional_at_least: 1,
            })),
        ),
        (
            &[0x31, 2, 0x52, 0x53][..],
            PacketViewError::Suffix(NeedMore {
                offset: 21,
                additional_at_least: 2,
            }),
        ),
        (
            &[0x31, 2, 0x52, 0x53, 0x64][..],
            PacketViewError::Suffix(NeedMore {
                offset: 22,
                additional_at_least: 1,
            }),
        ),
    ] {
        let error = <Packet<Segment> as WireView>::frame(input, 17)
            .err()
            .expect("truncated packet");
        match (error, expected) {
            (PacketViewError::Prefix(actual), PacketViewError::Prefix(expected))
            | (PacketViewError::Suffix(actual), PacketViewError::Suffix(expected)) => {
                assert_eq!(actual, expected);
            }
            (
                PacketViewError::Child(SegmentViewError::Length(actual)),
                PacketViewError::Child(SegmentViewError::Length(expected)),
            )
            | (
                PacketViewError::Child(SegmentViewError::Payload(actual)),
                PacketViewError::Child(SegmentViewError::Payload(expected)),
            ) => assert_eq!(actual, expected),
            (actual, expected) => panic!("wrong field context: {actual:?}, expected {expected:?}"),
        }
    }
}

#[test]
fn cursor_shortage_keeps_the_prior_parent_and_position() -> TestResult {
    let input = [0x31, 0, 0x64, 0x65, 0x32, 1, 0x52, 0x74];
    let (first, mut cursor) = Packet::<Segment>::cursor(&input)?;
    let error = Packet::<Segment>::next(&mut cursor)
        .err()
        .expect("missing suffix byte");
    assert!(matches!(
        error,
        wire_repr::SequenceError::Schema(PacketViewError::Suffix(NeedMore {
            offset: 8,
            additional_at_least: 1,
        }))
    ));
    assert_eq!(cursor.position(), 4);
    assert_eq!(cursor.remaining(), &input[4..]);
    assert_eq!(first.suffix(), 0x6465);
    Ok(())
}

#[test]
fn progressive_dynamic_setters_reject_an_unknown_offset_without_corrupting_output() -> TestResult {
    let mut output = [0xcc; 6];
    let writer = Packet::<Segment>::builder(&mut output[..]).prefix(0x31)?;
    let error = writer
        .suffix(0x6465)
        .err()
        .expect("child endpoint is not known");
    assert!(matches!(
        error,
        WriteError::Schema(PacketWriteError::Layout(wire_repr::LayoutError {
            field: "suffix"
        }))
    ));
    assert_eq!(output, [0x31, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc]);
    Ok(())
}

#[derive(WireView, WireBuilder)]
struct Item {
    value: u8,
}

#[derive(WireView, WireBuilder)]
struct MixedPacket {
    child: Segment,
    byte_length: u8,
    #[wire(bytes = byte_length)]
    bytes: wire::Bytes,
    item_count: i8,
    #[wire(counted_by = item_count)]
    items: wire::Array<Item>,
    suffix: u8,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum MixedEnvelope {
    #[wire(value = 0x67)]
    Record(MixedPacket),
}

#[test]
fn controllers_after_a_leading_child_patch_their_actual_offsets_in_both_writers() -> TestResult {
    for (child, bytes, items, expected) in [
        (&[][..], &[][..], &[][..], &[0, 0, 0, 0x51][..]),
        (
            &[0x21, 0x22][..],
            &[0x31, 0x32][..],
            &[0x41, 0x42][..],
            &[2, 0x21, 0x22, 2, 0x31, 0x32, 2, 0x41, 0x42, 0x51][..],
        ),
    ] {
        let mut output = [0xcc; 10];
        let written = MixedPacket::builder(&mut output[..])
            .child(|segment| segment.payload(child))?
            .bytes(bytes)?
            .items(|writer| {
                writer.try_extend(items.iter().copied(), |item, value| item.value(value))
            })?
            .suffix(0x51)?
            .finish()?;
        assert_eq!(written.as_bytes(), expected);
        let view = MixedPacket::view(written.as_bytes())?;
        assert_eq!(view.child().payload(), child);
        assert_eq!(view.byte_length(), bytes.len() as u8);
        assert_eq!(view.bytes(), bytes);
        assert_eq!(view.item_count(), items.len() as i8);
        assert_eq!(
            view.items()
                .iter()
                .map(|item| item.map(|item| item.view().value()))
                .collect::<Result<Vec<_>, _>>()?,
            items,
        );
        assert_eq!(view.suffix(), 0x51);

        let mut output = [0xcc; 11];
        let written = MixedEnvelope::builder(&mut output[..])
            .record(|packet| {
                packet
                    .child(|segment| segment.payload(child))
                    .bytes(bytes)
                    .items(|writer| {
                        writer.extend(items.iter().copied(), |item, value| item.value(value))
                    })
                    .suffix(0x51)
            })?
            .finish()?;
        assert_eq!(written.as_bytes()[0], 0x67);
        assert_eq!(&written.as_bytes()[1..], expected);
        let view = MixedEnvelope::view(written.as_bytes())?;
        let MixedEnvelopeVariant::Record(packet) = view.variant();
        assert_eq!(packet.byte_length(), bytes.len() as u8);
        assert_eq!(packet.item_count(), items.len() as i8);
        assert_eq!(packet.suffix(), 0x51);
    }
    Ok(())
}

#[test]
fn negative_count_after_nested_and_raw_geometry_fails_before_array_access() {
    let input = [2, 0x21, 0x22, 2, 0x31, 0x32, 0xff];
    for (error, offset) in [
        (
            MixedPacket::view_unchecked(input)
                .err()
                .expect("negative count"),
            6,
        ),
        (
            <MixedPacket as WireView>::frame(&input, 17)
                .err()
                .expect("negative count"),
            23,
        ),
    ] {
        assert!(matches!(
            error,
            MixedPacketViewError::ItemCountValue(wire_repr::ScalarConversionError {
                offset: actual,
                from: "i8",
                to: "usize",
            }) if actual == offset
        ));
    }
}

#[derive(WireView)]
struct Opaque {
    #[wire(rest)]
    bytes: wire::Bytes,
}

#[test]
fn nonleading_rest_child_still_requires_an_explicit_read_boundary() {
    assert!(matches!(
        Packet::<Opaque>::view([0x31, 0x52, 0x64, 0x65]),
        Err(PacketViewError::LayoutUnavailable { field: "child" })
    ));
}

#[test]
fn manual_leading_capabilities_frame_and_write_once_without_requiring_a_read_writer() -> TestResult
{
    use core::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    thread_local! {
        static FRAME_CALLS: Cell<usize> = const { Cell::new(0) };
    }

    struct Leading;
    struct NonLeading;
    struct WriteOnly;

    struct State {
        payload_length: usize,
    }

    struct ManualView<'a> {
        input: &'a [u8],
        payload_length: usize,
    }

    impl AsRef<[u8]> for ManualView<'_> {
        fn as_ref(&self) -> &[u8] {
            self.input
        }
    }

    impl ManualView<'_> {
        fn payload(&self) -> &[u8] {
            &self.input[1..1 + self.payload_length]
        }
    }

    // SAFETY: framing bounds the prefix and payload and retains only their length.
    // Reconstruction receives exactly that framed span and never keeps input in State.
    #[allow(unsafe_code)]
    unsafe impl WireView for Leading {
        type Error = NeedMore;
        type State = State;
        type View<'view> = ManualView<'view>;

        const FIXED_SIZE: Option<usize> = None;
        const LEADING_EXTENT: bool = true;

        fn frame(input: &[u8], offset: usize) -> Result<wire_repr::Frame<State>, NeedMore> {
            FRAME_CALLS.set(FRAME_CALLS.get() + 1);
            let length = *input.first().ok_or(NeedMore {
                offset,
                additional_at_least: 1,
            })? as usize;
            let consumed = 1 + length;
            if input.len() < consumed {
                return Err(NeedMore {
                    offset: offset + input.len(),
                    additional_at_least: consumed - input.len(),
                });
            }
            Ok(wire_repr::Frame::new(
                State {
                    payload_length: length,
                },
                consumed,
            ))
        }

        unsafe fn from_validated_parts<'view>(
            input: &'view [u8],
            state: &'view State,
        ) -> ManualView<'view> {
            ManualView {
                input,
                payload_length: state.payload_length,
            }
        }
    }

    // SAFETY: this terminal-only child retains no geometry and returns the exact framed slice.
    #[allow(unsafe_code)]
    unsafe impl WireView for NonLeading {
        type Error = Infallible;
        type State = ();
        type View<'view> = &'view [u8];

        const FIXED_SIZE: Option<usize> = None;

        fn frame(input: &[u8], _offset: usize) -> Result<wire_repr::Frame<()>, Infallible> {
            FRAME_CALLS.set(FRAME_CALLS.get() + 1);
            Ok(wire_repr::Frame::new((), input.len()))
        }

        unsafe fn from_validated_parts<'view>(
            input: &'view [u8],
            _state: &'view (),
        ) -> &'view [u8] {
            input
        }
    }

    struct BorrowedBytes<'a> {
        bytes: &'a [u8],
        writes: &'a AtomicUsize,
    }

    impl WireBuilder for Leading {
        type Builder = ();
        fn builder() {}
    }

    impl wire_repr::WireWrite<BorrowedBytes<'_>> for Leading {
        type Error = Infallible;

        fn write<O: wire_repr::Output>(
            value: BorrowedBytes<'_>,
            writer: &mut wire_repr::ChildWriter<'_, O>,
        ) -> Result<(), WriteError<Infallible, O::GrowError>> {
            value.writes.fetch_add(1, Ordering::Relaxed);
            writer.write(value.bytes)?;
            Ok(())
        }
    }

    impl WireBuilder for WriteOnly {
        type Builder = ();
        fn builder() {}
    }

    impl wire_repr::WireWrite<&[u8]> for WriteOnly {
        type Error = Infallible;

        fn write<O: wire_repr::Output>(
            value: &[u8],
            writer: &mut wire_repr::ChildWriter<'_, O>,
        ) -> Result<(), WriteError<Infallible, O::GrowError>> {
            writer.write(value)?;
            Ok(())
        }
    }

    let expected = [0x31, 2, 0x52, 0x53, 0x64, 0x65];
    let view = Packet::<Leading>::view(&expected[..])?;
    assert_eq!(FRAME_CALLS.get(), 1);
    for _ in 0..3 {
        assert_eq!(view.child().payload(), [0x52, 0x53]);
        assert_eq!(view.child().as_ref(), &expected[1..4]);
        assert_eq!(view.suffix(), 0x6465);
        let child = select(&view).include(|fields| fields.child);
        assert_eq!(child.chunks().collect::<Vec<_>>(), [&expected[1..4]]);
    }
    assert_eq!(FRAME_CALLS.get(), 1);
    assert!(matches!(
        Packet::<NonLeading>::view(expected),
        Err(PacketViewError::LayoutUnavailable { field: "child" })
    ));
    assert_eq!(FRAME_CALLS.get(), 1);

    let writes = AtomicUsize::new(0);
    let detached = <Packet<Leading> as WireBuilder>::builder()
        .prefix(0x31)
        .child(|()| BorrowedBytes {
            bytes: &expected[1..4],
            writes: &writes,
        })
        .suffix(0x6465);
    assert_eq!(writes.load(Ordering::Relaxed), 0);
    let mut output = [0xcc; 7];
    let written = Envelope::<Leading>::builder(&mut output[..])
        .record(|_| detached)?
        .finish()?;
    assert_eq!(written.as_bytes(), [0x45, 0x31, 2, 0x52, 0x53, 0x64, 0x65]);
    assert_eq!(writes.load(Ordering::Relaxed), 1);
    assert_eq!(FRAME_CALLS.get(), 1);

    let mut output = [0xcc; 6];
    let written = Packet::<Leading>::builder(&mut output[..])
        .prefix(0x31)?
        .child(|()| BorrowedBytes {
            bytes: &expected[1..4],
            writes: &writes,
        })?
        .suffix(0x6465)?
        .finish()?;
    assert_eq!(written.as_bytes(), expected);
    assert_eq!(writes.load(Ordering::Relaxed), 2);
    assert_eq!(FRAME_CALLS.get(), 1);

    let mut output = [0xcc; 6];
    let written = Packet::<WriteOnly>::builder(&mut output[..])
        .prefix(0x31)?
        .child(|()| &expected[1..4])?
        .suffix(0x6465)?
        .finish()?;
    assert_eq!(written.as_bytes(), expected);
    Ok(())
}

#[test]
fn invalid_child_extents_are_rejected_but_zero_width_leading_children_are_valid() -> TestResult {
    struct Manual<const OVERLONG: bool>;

    // SAFETY: reconstruction only returns its supplied slice. The overlong case deliberately
    // violates the extent contract without accessing any missing bytes, to exercise rejection.
    #[allow(unsafe_code)]
    unsafe impl<const OVERLONG: bool> WireView for Manual<OVERLONG> {
        type Error = Infallible;
        type State = ();
        type View<'view> = &'view [u8];

        const FIXED_SIZE: Option<usize> = None;
        const LEADING_EXTENT: bool = true;

        fn frame(input: &[u8], _offset: usize) -> Result<wire_repr::Frame<()>, Infallible> {
            let consumed = if OVERLONG { input.len() + 1 } else { 0 };
            Ok(wire_repr::Frame::new((), consumed))
        }

        unsafe fn from_validated_parts<'view>(
            input: &'view [u8],
            _state: &'view (),
        ) -> &'view [u8] {
            input
        }
    }

    impl WireBuilder for Manual<false> {
        type Builder = ();
        fn builder() {}
    }

    impl wire_repr::WireWrite<()> for Manual<false> {
        type Error = Infallible;

        fn write<O: wire_repr::Output>(
            (): (),
            _writer: &mut wire_repr::ChildWriter<'_, O>,
        ) -> Result<(), WriteError<Infallible, O::GrowError>> {
            Ok(())
        }
    }

    let error = <Packet<Manual<true>> as WireView>::frame(&[0x31, 0x52, 0x64, 0x65], 5)
        .err()
        .expect("overlong child");
    assert!(matches!(
        error,
        PacketViewError::ChildExtent(wire_repr::InvalidFrameExtent {
            offset: 6,
            consumed: 4,
            available: 3,
        })
    ));

    let view = Packet::<Manual<false>>::view([0x31, 0x64, 0x65])?;
    assert_eq!(view.child(), []);
    assert_eq!(view.suffix(), 0x6465);
    let mut output = [0xcc; 3];
    let written = Packet::<Manual<false>>::builder(&mut output[..])
        .prefix(0x31)?
        .child(|()| ())?
        .suffix(0x6465)?
        .finish()?;
    assert_eq!(written.as_bytes(), [0x31, 0x64, 0x65]);
    Ok(())
}

#[test]
fn a_child_failure_after_writing_keeps_typed_parent_context_and_partial_bytes() -> TestResult {
    struct Partial;

    #[derive(Debug, thiserror::Error)]
    #[error("manual payload rejected")]
    struct Rejected;

    impl WireBuilder for Partial {
        type Builder = ();
        fn builder() {}
    }

    impl wire_repr::WireWrite<()> for Partial {
        type Error = Rejected;

        fn write<O: wire_repr::Output>(
            (): (),
            writer: &mut wire_repr::ChildWriter<'_, O>,
        ) -> Result<(), WriteError<Rejected, O::GrowError>> {
            writer.write(&[0x52, 0x53])?;
            Err(WriteError::Schema(Rejected))
        }
    }

    let mut output = [0xcc; 6];
    let writer = Packet::<Partial>::builder(&mut output[..]).prefix(0x31)?;
    let error = writer.child(|()| ()).err().expect("manual write failure");
    assert!(matches!(
        error,
        WriteError::Schema(PacketWriteError::Child(Rejected))
    ));
    assert_eq!(output, [0x31, 0x52, 0x53, 0xcc, 0xcc, 0xcc]);

    let mut output = [0xcc; 7];
    let error = Envelope::<Partial>::builder(&mut output[..])
        .record(|packet| packet.prefix(0x31).child(|()| ()).suffix(0x6465))
        .err()
        .expect("detached manual write failure");
    assert!(matches!(
        error,
        WriteError::Schema(EnvelopeRecordWriteError::Body(PacketWriteError::Child(
            Rejected
        )))
    ));
    assert_eq!(output, [0x45, 0x31, 0x52, 0x53, 0xcc, 0xcc, 0xcc]);
    Ok(())
}

#[test]
fn output_shortages_at_parent_prefix_child_prefix_body_and_suffix_report_exact_capacity() {
    fn shortage<E: core::fmt::Debug>(
        error: WriteError<E, Infallible>,
        capacity: usize,
        minimum: usize,
    ) {
        match error {
            WriteError::Output(wire_repr::OutputError::NeedMore { request, available }) => {
                assert_eq!(available, capacity);
                assert_eq!(request.minimum_len, minimum);
            }
            error => panic!("wrong shortage error: {error:?}"),
        }
    }

    for capacity in 0..6 {
        let mut output = [0xcc; 6];
        let writer = match Packet::<Segment>::builder(&mut output[..capacity]).prefix(0x31) {
            Ok(writer) => writer,
            Err(error) => {
                assert_eq!(capacity, 0);
                shortage(error, capacity, 1);
                continue;
            }
        };
        let writer = match writer.child(|child| child.payload(&[0x52, 0x53][..])) {
            Ok(writer) => writer,
            Err(error) => {
                shortage(error, capacity, if capacity == 1 { 2 } else { 4 });
                assert_eq!(output[0], 0x31);
                assert!(output[capacity..].iter().all(|byte| *byte == 0xcc));
                continue;
            }
        };
        let error = writer
            .suffix(0x6465)
            .err()
            .expect("missing suffix capacity");
        shortage(error, capacity, 6);
        assert_eq!(&output[..4], &[0x31, 2, 0x52, 0x53]);
        assert!(output[capacity..].iter().all(|byte| *byte == 0xcc));
    }
}

mod delayed_roles {
    use super::*;

    #[derive(WireBuilder)]
    struct Detached<T> {
        prefix: u8,
        value: T,
        suffix: u8,
    }

    fn generated_value() -> u16 {
        0x3142
    }

    struct WriteOnly;

    impl WireBuilder for WriteOnly {
        type Builder = ();

        fn builder() {}
    }

    impl wire_repr::WireWrite<&[u8]> for WriteOnly {
        type Error = Infallible;

        fn write<O: wire_repr::Output>(
            value: &[u8],
            writer: &mut wire_repr::ChildWriter<'_, O>,
        ) -> Result<(), WriteError<Infallible, O::GrowError>> {
            writer.write(value)?;
            Ok(())
        }
    }

    #[derive(WireBuilder)]
    struct ComputedOnly<T> {
        child: T,
        #[wire(constant = 0x51)]
        before: u8,
        #[wire(be, computed = generated_value())]
        generated: u16,
        #[wire(constant = 0x62)]
        after: u8,
        suffix: u8,
    }

    fn selected_sum(selection: impl wire_repr::ByteSelection) -> u16 {
        selection.bytes().map(u16::from).sum()
    }

    #[derive(WireView, WireBuilder)]
    struct ComputedSelection {
        child: Segment,
        #[wire(constant = 0x51)]
        before: u8,
        #[wire(be, computed = selected_sum(include(child.payload, before, after, suffix)))]
        checksum: u16,
        #[wire(constant = 0x62)]
        after: u8,
        suffix: u8,
    }

    #[test]
    fn computed_suffixes_interleave_constants_in_write_only_and_selection_paths() -> TestResult {
        let child = [2, 0x21, 0x22];
        let mut output = [0xcc; 8];
        let written = ComputedOnly::<WriteOnly>::builder(&mut output[..])
            .child(|()| &child[..])?
            .suffix(0x73)?
            .finish()?;
        assert_eq!(
            written.as_bytes(),
            [2, 0x21, 0x22, 0x51, 0x31, 0x42, 0x62, 0x73]
        );

        let mut output = [0xcc; 10];
        let written = Detached::<ComputedOnly<WriteOnly>>::builder(&mut output[..])
            .prefix(0x84)?
            .value(|value| value.child(|()| &child[..]).suffix(0x73))?
            .suffix(0x95)?
            .finish()?;
        assert_eq!(
            written.as_bytes(),
            [0x84, 2, 0x21, 0x22, 0x51, 0x31, 0x42, 0x62, 0x73, 0x95],
        );

        let mut output = [0xcc; 8];
        let written = ComputedSelection::builder(&mut output[..])
            .child(|child| child.payload(&[0x21, 0x22][..]))?
            .suffix(0x73)?
            .finish()?;
        assert_eq!(
            written.as_bytes(),
            [2, 0x21, 0x22, 0x51, 1, 0x69, 0x62, 0x73]
        );
        let view = ComputedSelection::view(written.as_bytes())?;
        assert_eq!(view.checksum(), 0x0169);
        assert_eq!(view.before(), 0x51);
        assert_eq!(view.after(), 0x62);

        let mut output = [0xcc; 10];
        let written = Detached::<ComputedSelection>::builder(&mut output[..])
            .prefix(0x84)?
            .value(|value| {
                value
                    .child(|child| child.payload(&[0x21, 0x22][..]))
                    .suffix(0x73)
            })?
            .suffix(0x95)?
            .finish()?;
        assert_eq!(
            written.as_bytes(),
            [0x84, 2, 0x21, 0x22, 0x51, 1, 0x69, 0x62, 0x73, 0x95],
        );
        Ok(())
    }

    #[derive(WireView, WireBuilder)]
    struct ChoicePacket {
        child: Segment,
        #[wire(as = u8)]
        present: bool,
        #[wire(flag = present)]
        details: bool,
        #[wire(depends_on = details)]
        optional: u8,
        suffix: u8,
    }

    #[test]
    fn presence_choices_patch_the_controller_after_the_child_in_both_writers() -> TestResult {
        for (child, optional, expected) in [
            (&[][..], Some(0x41), &[0, 1, 0x41, 0x51][..]),
            (&[0x21, 0x22][..], None, &[2, 0x21, 0x22, 0, 0x51][..]),
        ] {
            let mut output = [0xcc; 5];
            let written = ChoicePacket::builder(&mut output[..])
                .child(|child_builder| child_builder.payload(child))?
                .details(|details| match optional {
                    Some(value) => details.present(|details| details.optional(value)),
                    None => details.absent(),
                })?
                .suffix(0x51)?
                .finish()?;
            assert_eq!(written.as_bytes(), expected);
            let view = ChoicePacket::view(written.as_bytes())?;
            assert_eq!(view.child().payload(), child);
            assert_eq!(view.present(), optional.is_some());
            assert_eq!(view.details(), optional.is_some());
            assert_eq!(view.optional(), optional);
            assert_eq!(view.suffix(), 0x51);

            let mut output = [0xcc; 7];
            let written = Detached::<ChoicePacket>::builder(&mut output[..])
                .prefix(0x62)?
                .value(|packet| {
                    packet
                        .child(|child_builder| child_builder.payload(child))
                        .details(|details| match optional {
                            Some(value) => details.present(|details| details.optional(value)),
                            None => details.absent(),
                        })
                        .suffix(0x51)
                })?
                .suffix(0x73)?
                .finish()?;
            assert_eq!(written.as_bytes()[0], 0x62);
            assert_eq!(&written.as_bytes()[1..1 + expected.len()], expected);
            assert_eq!(written.as_bytes()[1 + expected.len()], 0x73);
        }
        Ok(())
    }

    #[derive(WireView, WireBuilder)]
    struct PlacedFlags {
        child: Segment,
        raw: u8,
        #[wire(bits_of = raw, bit = 0)]
        enabled: bool,
        #[wire(bits_of = raw, bits = 1..=3)]
        kind: u8,
        offset: u8,
        #[wire(at = offset)]
        value: u8,
        suffix: u8,
    }

    #[test]
    fn inline_bits_and_placement_use_controllers_after_the_leading_child() -> TestResult {
        let input = [2, 0x21, 0x22, 0xcb, 8, 0xd1, 0xd2, 0xd3, 0x63, 0x74];
        let view = PlacedFlags::view(&input[..])?;
        assert_eq!(view.raw(), 0xcb);
        assert!(view.enabled());
        assert_eq!(view.kind(), 5);
        assert_eq!(view.offset(), 8);
        assert_eq!(view.value(), 0x63);
        assert_eq!(view.suffix(), 0x74);
        let selection = select(&view).include(|fields| fields.raw | fields.value);
        assert_eq!(
            selection.chunks().collect::<Vec<_>>(),
            [&input[3..4], &input[8..9]]
        );

        let mut output = [0xcc; 10];
        let written = PlacedFlags::builder(&mut output[..])
            .child(|child| child.payload(&[0x21, 0x22][..]))?
            .enabled(true)?
            .kind(5)?
            .offset(8)?
            .value(0x63)?
            .suffix(0x74)?
            .finish()?;
        assert_eq!(
            written.as_bytes(),
            [2, 0x21, 0x22, 0x0b, 8, 0, 0, 0, 0x63, 0x74]
        );

        let mut output = [0xcc; 12];
        let written = Detached::<PlacedFlags>::builder(&mut output[..])
            .prefix(0x85)?
            .value(|packet| {
                packet
                    .child(|child| child.payload(&[0x21, 0x22][..]))
                    .enabled(true)
                    .kind(5)
                    .offset(8)
                    .value(0x63)
                    .suffix(0x74)
            })?
            .suffix(0x96)?
            .finish()?;
        assert_eq!(
            written.as_bytes(),
            [0x85, 2, 0x21, 0x22, 0x0b, 8, 0, 0, 0, 0x63, 0x74, 0x96],
        );
        Ok(())
    }

    #[test]
    fn a_placement_controller_after_the_child_cannot_move_backwards() -> TestResult {
        assert!(matches!(
            PlacedFlags::view([2, 0x21, 0x22, 0x0b, 4, 0x63, 0x74]),
            Err(PlacedFlagsViewError::PositionBeforeCursor {
                field: "value",
                position: 4,
                cursor: 5,
            })
        ));
        let mut output = [0xcc; 10];
        let writer = PlacedFlags::builder(&mut output[..])
            .child(|child| child.payload(&[0x21, 0x22][..]))?
            .enabled(true)?
            .kind(5)?
            .offset(4)?;
        let error = writer.value(0x63).err().expect("backward placement");
        assert!(matches!(
            error,
            WriteError::Schema(PlacedFlagsWriteError::Layout(wire_repr::LayoutError {
                field: "value",
            }))
        ));
        assert_eq!(
            output,
            [2, 0x21, 0x22, 0x0b, 4, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc]
        );
        Ok(())
    }
}
