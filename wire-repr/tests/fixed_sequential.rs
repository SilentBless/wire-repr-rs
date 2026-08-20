#![deny(missing_docs, unsafe_code)]

//! Public fixed-sequential macro integration coverage.

use core::convert::Infallible;

use wire_repr::{FixedCodec, wire_repr};

/// A borrowed value returned by [`Borrowing`].
#[derive(Debug, PartialEq, Eq)]
pub struct BorrowedValue<'wire>(pub &'wire [u8]);

/// A two-byte codec that returns a borrowed value.
pub struct Borrowing;

impl FixedCodec for Borrowing {
    type Value<'wire>
        = BorrowedValue<'wire>
    where
        Self: 'wire;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 2]
    where
        Self: 'value;

    const WIDTH: usize = 2;

    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        BorrowedValue(bytes)
    }

    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([value.0[0], value.0[1]])
    }
}

/// A zero-width codec used to prove the early width rejection path.
pub struct ZeroWidth;

impl FixedCodec for ZeroWidth {
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

wire_repr! {
    /// Header exercises builtins whose declarations differ from physical order.
    pub layout Header {
        /// The big-endian code at physical position two.
        code @ 2: BeU16;
        /// The leading kind byte.
        kind @ 1: U8;
    }

    /// A view with a borrowed custom field and an arbitrary raw marker.
    pub layout Custom {
        /// The later raw marker.
        tracked @ 2: U8;
        /// The leading borrowed bytes.
        borrowed @ 1: crate::Borrowing;
    }

    /// A view with an invalid zero-width codec.
    pub layout Zero {
        /// The invalid codec.
        invalid @ 1: crate::ZeroWidth;
    }

    /// A crate-visible layout in the same macro invocation.
    pub(crate) layout CrateOnly {
        /// Its sole byte.
        value @ 1: U8;
    }

    /// A private layout in the same macro invocation.
    layout PrivateOnly {
        /// Its sole byte.
        value @ 1: U8;
    }
}

mod accepted_rendered_forms {
    wire_repr::wire_repr! {
        /// A restricted layout with a raw getter and qualified custom codec path.
        pub(in crate::accepted_rendered_forms) layout Restricted {
            /// The encoded type byte.
            r#type @ 1: U8;
        }
    }

    /// Parses through the restricted generated API.
    pub(super) fn value(input: &[u8]) -> Option<u8> {
        match Restricted::view(input).without_trailing() {
            Ok(view) => Some(view.r#type()),
            Err(_) => None,
        }
    }
}

#[test]
fn builtins_use_physical_bytes_and_exact_view_bytes() {
    let input = [0x7f, 0x12, 0x34];
    let view = Header::view(&input)
        .without_trailing()
        .expect("header should parse");
    assert_eq!(Header::WIDTH, 3);
    assert_eq!(view.kind(), 0x7f);
    assert_eq!(view.code(), 0x1234);
    assert_eq!(view.as_bytes(), input);
}

#[test]
fn prefix_keeps_the_original_suffix_and_excludes_it_from_the_view() {
    let input = [0x7f, 0x12, 0x34, 0xaa, 0xbb];
    let (view, suffix) = Header::view(&input)
        .with_remainder()
        .expect("header prefix should parse");
    assert_eq!(view.as_bytes(), &[0x7f, 0x12, 0x34]);
    assert_eq!(suffix, &[0xaa, 0xbb]);
    assert_eq!(view.as_bytes().as_ptr(), input.as_ptr());
    assert_eq!(suffix.as_ptr(), input[3..].as_ptr());
}

#[test]
fn fluent_immutable_terminals_preserve_framing_and_copy_semantics() {
    fn assert_copy_clone<T: Copy + Clone>() {}

    let input = [0x7f, 0x12, 0x34, 0xaa];
    let (view, suffix) = Header::view(&input).with_remainder().expect("valid prefix");
    assert_copy_clone::<Header<'_>>();
    let copied = view;
    assert_eq!(copied.as_bytes(), &input[..3]);
    assert_eq!(view.as_bytes(), &input[..3]);
    assert_eq!(suffix, &[0xaa]);
    assert!(matches!(
        Header::view(&input).without_trailing(),
        Err(HeaderError::TrailingBytes {
            expected: 3,
            actual: 4
        })
    ));
    assert_eq!(
        Header::view(&input[..3])
            .without_trailing()
            .unwrap()
            .as_bytes(),
        &input[..3]
    );
}

#[test]
fn exact_parsing_reports_short_and_trailing_inputs() {
    assert!(matches!(
        Header::view(&[0x7f, 0x12]).without_trailing(),
        Err(HeaderError::InputTooShort {
            expected: 3,
            actual: 2
        })
    ));
    assert!(matches!(
        Header::view(&[0x7f, 0x12, 0x34, 0]).without_trailing(),
        Err(HeaderError::TrailingBytes {
            expected: 3,
            actual: 4
        })
    ));
}

#[test]
fn custom_fields_borrow_and_arbitrary_raw_markers_structurally_parse() {
    let input = [0xca, 0xfe, 0xff];
    let view = Custom::view(&input)
        .without_trailing()
        .expect("exact-width raw marker should parse");
    assert_eq!(view.borrowed(), BorrowedValue(&[0xca, 0xfe]));
    assert_eq!(view.borrowed().0.as_ptr(), input.as_ptr());
    assert_eq!(view.tracked(), 0xff);
    assert!(matches!(view.tracked(), 0xff));
}

#[test]
fn zero_width_is_rejected_before_input() {
    assert!(matches!(
        Zero::view(&[]).with_remainder(),
        Err(ZeroError::InvalidCodecWidth { position: 1 })
    ));
}

#[test]
fn multiple_layout_visibilities_and_runtime_facades_remain_usable() {
    assert_eq!(accepted_rendered_forms::value(&[0x2a]), Some(0x2a));
    assert!(CrateOnly::view(&[1]).without_trailing().is_ok());
    assert!(PrivateOnly::view(&[2]).without_trailing().is_ok());
    assert_eq!(<wire_repr::BeU16 as wire_repr::FixedCodec>::WIDTH, 2);
    assert_eq!(
        <wire_repr::codec::BeU16 as wire_repr::codec::FixedCodec>::WIDTH,
        2
    );
}
