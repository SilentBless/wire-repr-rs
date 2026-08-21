#![deny(missing_docs, unsafe_code)]
//! Public fixed sequential derive coverage.

use wire_repr::{FixedCodec, PreparedLayout, Wire};

/// A custom fixed codec which rejects zero during planning.
pub struct NonZero;

/// Custom codec error.
#[derive(Debug, Eq, PartialEq)]
pub enum NonZeroError {
    /// Zero is rejected.
    Zero,
}

impl core::fmt::Display for NonZeroError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("zero")
    }
}

impl core::error::Error for NonZeroError {}

impl FixedCodec for NonZero {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type EncodeError = NonZeroError;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;

    const WIDTH: usize = 1;

    fn decode(bytes: &[u8]) -> u8 {
        bytes[0]
    }

    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        if value == 0 {
            Err(NonZeroError::Zero)
        } else {
            Ok([value])
        }
    }
}

/// A deliberately non-Copy semantic value.
#[derive(Debug, Eq, PartialEq)]
pub struct NonCopyValue(u8);

/// A fixed codec for [`NonCopyValue`].
pub struct NonCopy;

impl FixedCodec for NonCopy {
    type Value<'wire>
        = NonCopyValue
    where
        Self: 'wire;
    type EncodeError = core::convert::Infallible;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;

    const WIDTH: usize = 1;

    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        NonCopyValue(bytes[0])
    }

    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([value.0])
    }
}

/// A header whose semantic field cannot be copied or cloned.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct OwnedHeader {
    /// Owned semantic marker.
    #[wire(codec = NonCopy)]
    pub marker: NonCopyValue,
}

/// A fixed header using native Rust byte arrays.
#[derive(Debug, Eq, PartialEq, Wire)]
pub struct ArrayHeader {
    /// Opaque fixed-width bytes.
    pub digest: [u8; 4],
    /// Trailing marker.
    pub marker: u8,
}

/// A fixed wire header retained as a semantic write value.
#[derive(Debug, PartialEq, Eq, Wire)]
pub struct Header {
    /// Kind byte.
    pub kind: u8,
    /// Network order code.
    #[wire(be)]
    pub code: u16,
    /// Custom marker.
    #[wire(codec = NonZero)]
    pub marker: u8,
}

#[test]
fn frames_with_or_without_trailing_bytes_into_bytes_backed_getters() {
    let original = Header {
        kind: 1,
        code: 0x1234,
        marker: 7,
    };
    assert_eq!(original.code, 0x1234);

    let parsed = Header::view(&[1, 0x12, 0x34, 7])
        .without_trailing()
        .unwrap();
    assert_eq!(parsed.kind(), 1);
    assert_eq!(parsed.code(), 0x1234);
    assert_eq!(parsed.marker(), 7);
    assert_eq!(parsed.as_bytes(), &[1, 0x12, 0x34, 7]);

    let (prefix, suffix) = Header::view(&[2, 0xab, 0xcd, 9, 42])
        .with_remainder()
        .unwrap();
    assert_eq!(prefix.kind(), 2);
    assert_eq!(prefix.code(), 0xabcd);
    assert_eq!(prefix.marker(), 9);
    assert_eq!(prefix.as_bytes(), &[2, 0xab, 0xcd, 9]);
    assert_eq!(suffix, &[42]);

    let error = Header::view(&[2, 0xab]).without_trailing().unwrap_err();
    assert_eq!(
        error.to_string(),
        "field `code` needs 2 bytes, but only 1 byte remains"
    );

    let error = Header::view(&[2, 0xab, 0xcd, 9, 42])
        .without_trailing()
        .unwrap_err();
    assert!(matches!(
        error,
        HeaderDecodeError::TrailingBytes {
            expected: 4,
            actual: 5,
        }
    ));
    assert_eq!(
        error.to_string(),
        "input has 1 trailing byte after the 4-byte representation"
    );
}

#[test]
fn plan_commit_suffix_and_errors_are_atomic() {
    let header = Header {
        kind: 3,
        code: 0x0102,
        marker: 4,
    };
    let plan = header.prepare().unwrap();
    assert_eq!(plan.encoded_len(), 4);

    let mut output = [0_u8; 6];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[3, 1, 2, 4]);
    assert_eq!(suffix, &mut [0, 0]);

    let mut short = [0xa5; 3];
    let short_header = Header {
        kind: 3,
        code: 0x0102,
        marker: 4,
    };
    assert!(short_header.build_into(&mut short).is_err());
    assert_eq!(short, [0xa5; 3]);

    let bad = Header {
        kind: 1,
        code: 2,
        marker: 0,
    };
    let error = match bad.prepare() {
        Err(error) => error,
        Ok(_) => panic!("zero marker should be rejected"),
    };
    assert!(matches!(
        error,
        HeaderEncodeError::Marker(NonZeroError::Zero)
    ));
    assert_eq!(
        error.to_string(),
        "wire preparation failed for field `marker`: Zero"
    );
    assert_eq!(
        HeaderEncodeError::LengthOverflow.to_string(),
        "encoded representation length does not fit in usize"
    );
}

#[test]
fn custom_fixed_getters_decode_non_copy_values_without_constructing_header() {
    let parsed = OwnedHeader::view(&[9]).without_trailing().unwrap();
    assert_eq!(parsed.marker(), NonCopyValue(9));
    assert_eq!(parsed.as_bytes(), &[9]);

    let plan = OwnedHeader {
        marker: NonCopyValue(7),
    }
    .prepare()
    .unwrap();
    let mut output = [0_u8; 1];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();

    assert_eq!(written.as_bytes(), &[7]);
    assert!(suffix.is_empty());
}

#[test]
fn native_byte_arrays_borrow_the_validated_wire_storage() {
    let input = [1, 2, 3, 4, 9, 0xaa];
    let (parsed, suffix) = ArrayHeader::view(&input).with_remainder().unwrap();
    let digest: &[u8; 4] = parsed.digest();
    assert_eq!(digest, &[1, 2, 3, 4]);
    assert_eq!(parsed.marker(), 9);
    assert_eq!(parsed.as_bytes(), &input[..5]);
    assert_eq!(suffix, &input[5..]);

    let plan = ArrayHeader {
        digest: [5, 6, 7, 8],
        marker: 10,
    }
    .prepare()
    .unwrap();
    let mut output = [0_u8; 5];
    let (written, suffix) = plan.commit_into(&mut output).unwrap();
    assert_eq!(written.as_bytes(), &[5, 6, 7, 8, 10]);
    assert!(suffix.is_empty());
}

#[test]
fn fixed_sequences_validate_once_then_iterate_infallibly() {
    let bytes = [1, 0x12, 0x34, 7, 2, 0xab, 0xcd, 9];
    let mut headers = Header::views(&bytes).unwrap();
    assert_eq!(headers.len(), 2);

    let first = headers.next().unwrap();
    assert_eq!(first.kind(), 1);
    assert_eq!(first.code(), 0x1234);
    assert_eq!(first.marker(), 7);
    let second = headers.next().unwrap();
    assert_eq!(second.kind(), 2);
    assert_eq!(second.code(), 0xabcd);
    assert_eq!(second.marker(), 9);
    assert!(headers.next().is_none());

    assert_eq!(
        Header::views(&bytes[..7]).unwrap_err(),
        wire_repr::FixedViewSequenceError::TrailingPartialItem {
            item_width: 4,
            trailing: 3,
        }
    );
}
