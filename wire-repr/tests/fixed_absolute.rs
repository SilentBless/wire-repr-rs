#![deny(missing_docs, unsafe_code)]

//! Public fixed-absolute macro integration coverage.

use core::convert::Infallible;

use wire_repr::{FixedCodec, wire_repr};

/// A borrowed two-byte value.
#[derive(Debug, PartialEq, Eq)]
pub struct Borrowed<'wire>(pub &'wire [u8]);

/// A codec with a borrowed decoded value.
pub struct Borrowing;

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

/// An invalid zero-width codec.
pub struct Zero;

impl FixedCodec for Zero {
    type Value<'wire>
        = ()
    where
        Self: 'wire;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 0]
    where
        Self: 'value;

    const WIDTH: usize = 0;

    fn decode<'wire>(_: &'wire [u8]) -> Self::Value<'wire> {}

    fn plan<'value>(_: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([])
    }
}

/// A codec with an overflowing declared extent at nonzero offset.
pub struct Overflow;

impl FixedCodec for Overflow {
    type Value<'wire>
        = ()
    where
        Self: 'wire;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 0]
    where
        Self: 'value;

    const WIDTH: usize = usize::MAX;

    fn decode<'wire>(_: &'wire [u8]) -> Self::Value<'wire> {}

    fn plan<'value>(_: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([])
    }
}

/// A three-byte codec used to create runtime overlap.
pub struct Wide;

impl FixedCodec for Wide {
    type Value<'wire>
        = ()
    where
        Self: 'wire;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 3]
    where
        Self: 'value;

    const WIDTH: usize = 3;

    fn decode<'wire>(_: &'wire [u8]) -> Self::Value<'wire> {}

    fn plan<'value>(_: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([0; 3])
    }
}

wire_repr! {
    /// Builtin offsets are intentionally not declaration order.
    pub absolute layout Header {
        /// The trailing code.
        field tail: BeU16 { offset: 4; }
        /// The leading kind.
        field kind: U8 { offset: 0; }
    }

    /// A borrowed field and an arbitrary raw marker.
    pub absolute layout Custom {
        /// The borrowed middle bytes.
        field borrowed: codec(Borrowing) { offset: 1; }
        /// The leading raw marker.
        field tracked: U8 { offset: 0; }
    }

    /// The zero-width configuration case.
    pub absolute layout ZeroLayout {
        /// Invalid field.
        field invalid: codec(Zero) { offset: 2; }
    }

    /// The overflowing configuration case.
    pub absolute layout OverflowLayout {
        /// Invalid field.
        field invalid: codec(Overflow) { offset: 1; }
    }

    /// The overlap configuration case.
    pub absolute layout OverlapLayout {
        /// Earlier wide field.
        field wide: codec(Wide) { offset: 0; }
        /// Later overlapping field.
        field later: U8 { offset: 2; }
    }

    /// Sequential and absolute declarations can share an invocation.
    pub layout SequentialHere {
        /// Value.
        field value: U8 { position: 1; }
    }

    /// Absolute companion declaration in the same invocation.
    pub absolute layout AbsoluteHere {
        /// Value.
        field value: U8 { offset: 0; }
    }
}

#[test]
fn builtins_use_absolute_offsets_and_preserve_gaps() {
    let input = [0x7f, 0xaa, 0xbb, 0xcc, 0x12, 0x34];
    let view = HeaderView::parse_exact(&input).expect("absolute header should parse");
    assert_eq!(HeaderView::WIDTH, 6);
    assert_eq!(view.tail(), 0x1234);
    assert_eq!(view.kind(), 0x7f);
    assert_eq!(view.as_bytes(), input);
}

#[test]
fn prefix_and_exact_errors_use_maximum_extent() {
    let input = [0x7f, 0xaa, 0xbb, 0xcc, 0x12, 0x34, 0xee];
    let (view, suffix) =
        HeaderView::parse_prefix(&input).expect("absolute header prefix should parse");
    assert_eq!(view.as_bytes().as_ptr(), input.as_ptr());
    assert_eq!(suffix.as_ptr(), input[6..].as_ptr());
    assert_eq!(suffix, &[0xee]);
    assert!(matches!(
        HeaderView::parse_exact(&input),
        Err(HeaderError::TrailingBytes {
            expected: 6,
            actual: 7
        })
    ));
    assert!(matches!(
        HeaderView::parse_exact(&input[..5]),
        Err(HeaderError::InputTooShort {
            expected: 6,
            actual: 5
        })
    ));
}

#[test]
fn custom_fields_borrow_and_arbitrary_raw_markers_structurally_parse() {
    let input = [0xff, 0xca, 0xfe];
    let view = CustomView::parse_exact(&input).expect("exact-width raw marker should parse");
    assert_eq!(view.borrowed(), Borrowed(&[0xca, 0xfe]));
    assert_eq!(view.borrowed().0.as_ptr(), input[1..].as_ptr());
    assert_eq!(view.tracked(), 0xff);
    assert!(matches!(view.tracked(), 0xff));
}

#[test]
fn configuration_errors_precede_input_in_offset_order() {
    assert!(matches!(
        ZeroLayoutView::parse_prefix(&[]),
        Err(ZeroLayoutError::InvalidCodecWidth { offset: 2 })
    ));
    assert!(
        matches!(OverflowLayoutView::parse_prefix(&[]), Err(OverflowLayoutError::InvalidCodecExtent { offset: 1, width }) if width == usize::MAX)
    );
    assert!(matches!(
        OverlapLayoutView::parse_prefix(&[]),
        Err(OverlapLayoutError::OverlappingFields {
            earlier_offset: 0,
            later_offset: 2
        })
    ));
}

#[test]
fn mixed_layout_modes_remain_public_through_the_facade() {
    assert!(SequentialHereView::parse_exact(&[1]).is_ok());
    assert!(AbsoluteHereView::parse_exact(&[2]).is_ok());
    assert_eq!(<wire_repr::BeU16 as wire_repr::FixedCodec>::WIDTH, 2);
}
