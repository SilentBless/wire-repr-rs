use core::convert::Infallible;
use core::num::NonZeroUsize;
use core::sync::atomic::{AtomicUsize, Ordering};

use wire_repr::{PrefixCodec, PrefixExtent, wire_repr};

static LENGTH_VALIDATIONS: AtomicUsize = AtomicUsize::new(0);
static LENGTH_DECODE_BYTES: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LengthError {
    Empty,
    Incomplete,
}

struct LengthPrefix;

impl PrefixCodec for LengthPrefix {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type DecodeError = LengthError;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;

    fn validate_prefix(bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        LENGTH_VALIDATIONS.fetch_add(1, Ordering::Relaxed);
        match bytes {
            [] => Err(LengthError::Empty),
            [0] => Err(LengthError::Incomplete),
            [0, _, ..] => Ok(PrefixExtent::new(NonZeroUsize::new(2).unwrap())),
            [_, ..] => Ok(PrefixExtent::new(NonZeroUsize::MIN)),
        }
    }

    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        LENGTH_DECODE_BYTES.store(bytes.len(), Ordering::Relaxed);
        match bytes {
            [0, value] => *value,
            [value] => value - 1,
            _ => panic!("decode must receive one exact validated prefix"),
        }
    }

    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([value + 1])
    }
}

struct PlainLengthPrefix;

impl PrefixCodec for PlainLengthPrefix {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type DecodeError = LengthError;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;

    fn validate_prefix(bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        match bytes {
            [] => Err(LengthError::Empty),
            [0] => Err(LengthError::Incomplete),
            [0, _, ..] => Ok(PrefixExtent::new(NonZeroUsize::new(2).unwrap())),
            [_, ..] => Ok(PrefixExtent::new(NonZeroUsize::MIN)),
        }
    }

    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        match bytes {
            [0, value] => *value,
            [value] => value - 1,
            _ => panic!("decode must receive one exact validated prefix"),
        }
    }

    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        Ok([value + 1])
    }
}

wire_repr! {
    pub layout Framed {
        field tail: U8 { position: 6; }
        align { position: 5; boundary: 4; }
        field second: region(length) { position: 4; }
        padding { position: 3; length: 1; }
        /// The first opaque region.
        field first: region(length) { position: 2; }
        field length: prefix(crate::LengthPrefix) { position: 1; }
    }

    pub layout EmptyRegions {
        field tail: U8 { position: 4; }
        field second: region(length) { position: 3; }
        field first: region(length) { position: 2; }
        field length: U8 { position: 1; }
    }

    pub layout WideLength {
        field payload: region(length) { position: 2; }
        field length: BeU128 { position: 1; }
    }

    pub layout ShortFramed {
        field payload: region(length) { position: 2; }
        field length: prefix(crate::PlainLengthPrefix) { position: 1; }
    }
}

#[test]
fn prefix_lengths_frame_exact_regions_without_revalidation_or_reencoding() {
    LENGTH_VALIDATIONS.store(0, Ordering::Relaxed);
    LENGTH_DECODE_BYTES.store(0, Ordering::Relaxed);

    let noncanonical = [0, 2, b'a', b'b', 0xee, b'c', b'd', 0xff, 9, 0xaa];
    let (view, suffix) = FramedView::parse_prefix(&noncanonical).unwrap();

    assert_eq!(view.as_bytes(), &noncanonical[..9]);
    assert_eq!(suffix, &[0xaa]);
    assert_eq!(view.length_encoded(), &[0, 2]);
    assert_eq!(view.first(), b"ab");
    assert_eq!(view.second(), b"cd");
    assert_eq!(view.tail(), 9);
    assert_eq!(LENGTH_VALIDATIONS.load(Ordering::Relaxed), 1);
    assert_eq!(LENGTH_DECODE_BYTES.load(Ordering::Relaxed), 2);

    let canonical = [3, b'a', b'b', 0xee, b'c', b'd', 0xf0, 0xf1, 9];
    let view = FramedView::parse_exact(&canonical).unwrap();
    assert_eq!(view.length_encoded(), &[3]);
    assert_eq!(view.first(), b"ab");
    assert_eq!(view.second(), b"cd");
    assert_eq!(view.tail(), 9);
}

#[test]
fn adjacent_zero_length_regions_are_exact_and_do_not_stall_physical_progress() {
    let bytes = [0, 9];
    let view = EmptyRegionsView::parse_exact(&bytes).unwrap();

    assert_eq!(view.length(), 0);
    assert!(view.first().is_empty());
    assert!(view.second().is_empty());
    assert_eq!(view.tail(), 9);
    assert_eq!(view.as_bytes(), &bytes);
}

#[test]
fn conversion_and_shortage_errors_precede_later_physical_entries() {
    let too_wide = [0xff; 16];
    assert!(matches!(
        WideLengthView::parse_prefix(&too_wide),
        Err(WideLengthError::InvalidRegionLength {
            position: 2,
            source_position: 1,
        })
    ));

    let short = [3, b'a'];
    assert!(matches!(
        ShortFramedView::parse_prefix(&short),
        Err(ShortFramedError::InputTooShort {
            position: 2,
            expected: 2,
            available: 1,
        })
    ));

    assert!(matches!(
        ShortFramedView::parse_prefix(&[]),
        Err(ShortFramedError::FieldLength(LengthError::Empty))
    ));
}
