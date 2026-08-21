#![deny(missing_docs, unsafe_code)]
//! Borrowed terminal field derive coverage.

use wire_repr::{PreparedLayout, Wire};

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
    let parsed = Packet::view(&input).without_trailing().unwrap();

    assert_eq!(parsed.as_bytes(), &input);
    assert_eq!(parsed.kind(), 7);
    assert_eq!(parsed.sequence(), 0x1234);
    assert_eq!(parsed.payload(), &input[3..]);
    assert!(core::ptr::eq(
        parsed.payload().as_ptr(),
        input[3..].as_ptr()
    ));

    let (parsed, suffix) = Packet::view(&input).with_remainder().unwrap();
    assert_eq!(parsed.as_bytes(), &input);
    assert!(suffix.is_empty());
}

#[test]
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
