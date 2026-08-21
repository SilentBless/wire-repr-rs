#![deny(missing_docs, unsafe_code)]
//! Nested `Wire` derive composition coverage.

use wire_repr::{PreparedLayout, Wire};

/// A child representation between parent fixed fields.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct Child {
    /// The child kind.
    pub kind: u8,
    /// The child network-order value.
    #[wire(be)]
    pub value: u16,
}

/// A parent with fixed siblings and two independent nested children.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct Parent {
    /// The leading fixed value.
    pub lead: u8,
    /// The first nested value.
    pub first: Child,
    /// The fixed separator.
    pub separator: i8,
    /// The second nested value.
    pub second: Child,
    /// The trailing fixed value.
    pub tail: u8,
}

/// A nested child borrowing a bounded payload.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct BorrowedChild<'wire> {
    /// Encoded payload length.
    pub length: u8,
    /// Borrowed child payload.
    #[wire(bytes = length)]
    pub payload: &'wire [u8],
}

/// A parent retaining the same input lifetime through its nested child.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct BorrowedParent<'value> {
    /// Leading marker.
    pub lead: u8,
    /// Borrowed nested representation.
    pub child: BorrowedChild<'value>,
    /// Trailing marker after the bounded child.
    pub tail: u8,
}

#[test]
fn nested_decode_preserves_bytes_suffix_and_child_provenance() {
    let bytes = [1, 2, 0, 3, -4_i8 as u8, 5, 0, 6, 7, 99];
    let (parsed, suffix) = Parent::view(&bytes).with_remainder().unwrap();
    assert_eq!(parsed.as_bytes(), &bytes[..9]);
    assert_eq!(suffix, &[99]);
    assert_eq!(parsed.lead(), 1);
    let first = parsed.first();
    assert_eq!(first.kind(), 2);
    assert_eq!(first.value(), 3);
    assert_eq!(first.as_bytes(), &bytes[1..4]);
    assert!(core::ptr::eq(
        first.as_bytes().as_ptr(),
        bytes[1..4].as_ptr()
    ));
    assert_eq!(parsed.separator(), -4);
    let second = parsed.second();
    assert_eq!(second.kind(), 5);
    assert_eq!(second.value(), 6);
    assert_eq!(second.as_bytes(), &bytes[5..8]);
    assert!(core::ptr::eq(
        second.as_bytes().as_ptr(),
        bytes[5..8].as_ptr()
    ));
    assert_eq!(parsed.tail(), 7);
    let copied = parsed;
    assert_eq!(copied.first().as_bytes(), first.as_bytes());
    assert!(matches!(
        Parent::view(&bytes).without_trailing(),
        Err(ParentDecodeError::TrailingBytes {
            expected: 9,
            actual: 10
        })
    ));
    let error = Parent::view(&[1, 2]).without_trailing().unwrap_err();
    assert!(matches!(
        error,
        ParentDecodeError::First(ChildDecodeError::InputTooShort {
            field: "value",
            required: 2,
            available: 0
        })
    ));
    assert_eq!(
        error.to_string(),
        "wire decode failed in field `first`: field `value` needs 2 bytes, but only 0 bytes remain"
    );
}

#[test]
fn nested_prepare_commit_is_exact_and_atomic() {
    let parent = Parent {
        lead: 1,
        first: Child { kind: 2, value: 3 },
        separator: -4,
        second: Child { kind: 5, value: 6 },
        tail: 7,
    };
    let plan = parent.prepare().unwrap();
    assert_eq!(plan.encoded_len(), 9);
    let mut output = [0_u8; 11];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[1, 2, 0, 3, 252, 5, 0, 6, 7]);
    assert_eq!(suffix, &mut [0, 0]);

    let mut short = [0xa5; 8];
    let parent = Parent {
        lead: 1,
        first: Child { kind: 2, value: 3 },
        separator: -4,
        second: Child { kind: 5, value: 6 },
        tail: 7,
    };
    assert!(parent.build_into(&mut short).is_err());
    assert_eq!(short, [0xa5; 8]);
}

#[test]
fn borrowed_nested_values_preserve_one_input_lifetime() {
    let input = [1, 2, 7, 8, 9, 0xaa];
    let (parsed, suffix) = BorrowedParent::view(&input).with_remainder().unwrap();
    assert_eq!(parsed.as_bytes(), &input[..5]);
    assert_eq!(parsed.lead(), 1);
    let child = parsed.child();
    assert_eq!(child.length(), 2);
    assert_eq!(child.payload(), &input[2..4]);
    assert!(core::ptr::eq(
        child.payload().as_ptr(),
        input[2..4].as_ptr()
    ));
    assert_eq!(child.as_bytes(), &input[1..4]);
    assert!(core::ptr::eq(
        child.as_bytes().as_ptr(),
        input[1..4].as_ptr()
    ));
    assert_eq!(parsed.tail(), 9);
    assert_eq!(suffix, &[0xaa]);

    let payload = [4, 5, 6];
    let plan = BorrowedParent {
        lead: 2,
        child: BorrowedChild {
            length: 99,
            payload: &payload,
        },
        tail: 3,
    }
    .prepare()
    .unwrap();
    let mut output = [0_u8; 7];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[2, 3, 4, 5, 6, 3]);
    assert_eq!(suffix, &mut [0]);
}
