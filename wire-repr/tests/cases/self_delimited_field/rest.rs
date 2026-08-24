#![deny(missing_docs, unsafe_code)]
//! Borrowed terminal field derive coverage.

#[cfg(not(feature = "bytes"))]
use wire_repr::PreparedLayout;
use wire_repr::{ViewCursorError, Wire};

/// A packet with a fixed header and borrowed terminal payload.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct Packet<'wire> {
    /// Packet kind.
    pub kind: u8,
    /// Network-order sequence.
    #[wire(be)]
    pub sequence: u16,
    /// Remaining represented bytes.
    #[wire(rest)]
    pub payload: &'wire [u8],
}

#[test]
fn terminal_slice_borrows_exact_input_and_consumes_the_frame() {
    let input = [7, 0x12, 0x34, 1, 2, 3];
    let parsed = Packet::view(&input).unwrap();

    assert_eq!(parsed.as_bytes(), &input);
    assert_eq!(parsed.kind(), 7);
    assert_eq!(parsed.sequence(), 0x1234);
    assert_eq!(parsed.payload(), &input[3..]);
    assert!(core::ptr::eq(
        parsed.payload().as_ptr(),
        input[3..].as_ptr()
    ));
}

#[test]
fn terminal_slice_retains_arbitrary_owned_backing() {
    struct Backing(Vec<u8>);

    impl AsRef<[u8]> for Backing {
        fn as_ref(&self) -> &[u8] {
            &self.0
        }
    }

    fn inspect(view: &impl PacketView) {
        assert_eq!(view.kind(), 7);
        assert_eq!(view.sequence(), 0x1234);
        assert_eq!(view.payload(), &[1, 2, 3]);
    }

    let backing = Backing(vec![7, 0x12, 0x34, 1, 2, 3]);
    let pointer = backing.0.as_ptr();
    let parsed = Packet::view(backing).unwrap();

    inspect(&parsed);
    assert_eq!(parsed.as_bytes().as_ptr(), pointer);
}

#[test]
fn terminal_slice_selection_uses_validated_field_ranges() {
    let input = [7, 0x12, 0x34, 1, 2, 3];
    let parsed = Packet::view(&input).unwrap();
    let selected = PacketView::bytes(&parsed).include(|fields| fields.payload | fields.kind);
    let mut output = [0; 4];

    assert_eq!(selected.byte_len(), 4);
    selected.write_into(&mut output);
    assert_eq!(output, [7, 1, 2, 3]);
}

#[test]
fn terminal_slice_cursor_is_fail_closed() {
    let short = [7, 0x12];
    assert!(matches!(
        Packet::view(&short),
        Err(PacketDecodeError::InputTooShort { .. })
    ));
    let mut cursor = Packet::cursor(&short);

    assert!(matches!(
        cursor.next(),
        Err(ViewCursorError::Item(
            PacketDecodeError::InputTooShort { .. }
        ))
    ));
    assert_eq!(cursor.remaining(), &short);

    let input = [7, 0x12, 0x34, 1, 2, 3];
    let mut cursor = Packet::cursor(&input);
    let parsed = cursor.next().unwrap().unwrap();
    assert_eq!(parsed.as_bytes(), &input);
    assert!(cursor.remaining().is_empty());
    assert!(cursor.next().unwrap().is_none());

    let mut unchecked = Packet::cursor(&input).unchecked();
    assert_eq!(unchecked.next().unwrap().unwrap().payload(), &[1, 2, 3]);
    assert!(unchecked.remaining().is_empty());
}

#[cfg(feature = "bytes")]
#[test]
fn terminal_slice_retains_bytes_storage() {
    let backing = bytes::Bytes::from_static(&[7, 0x12, 0x34, 1, 2, 3]);
    let pointer = backing.as_ptr();
    let parsed = Packet::view(backing).unwrap();

    assert_eq!(parsed.as_bytes().as_ptr(), pointer);
    assert_eq!(parsed.payload().as_ptr(), pointer.wrapping_add(3));
}

#[test]
#[cfg(not(feature = "bytes"))]
fn terminal_slice_prepares_and_commits_atomically() {
    let payload = [4, 5, 6];
    let packet = Packet {
        kind: 9,
        sequence: 0x1234,
        payload: &payload,
    };
    let plan = packet.prepare().unwrap();
    assert_eq!(plan.encoded_len(), 6);

    let mut output = [0_u8; 8];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[9, 0x12, 0x34, 4, 5, 6]);
    assert_eq!(suffix, &mut [0, 0]);

    let mut short = [0xa5; 5];
    let packet = Packet {
        kind: 9,
        sequence: 0x1234,
        payload: &payload,
    };
    assert!(packet.build_into(&mut short).is_err());
    assert_eq!(short, [0xa5; 5]);
}
