use core::{convert::Infallible, num::NonZeroUsize};

use wire_repr::{PrefixCodec, PrefixExtent, wire_repr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TinyError {
    Incomplete,
}

struct TinyPrefix;

impl PrefixCodec for TinyPrefix {
    type Value<'wire>
        = u8
    where
        Self: 'wire;
    type DecodeError = TinyError;
    type EncodeError = Infallible;
    type Plan<'value>
        = [u8; 1]
    where
        Self: 'value;

    fn validate_prefix(bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError> {
        match bytes {
            [0, _, ..] => Ok(PrefixExtent::new(NonZeroUsize::new(2).unwrap())),
            [_, ..] => Ok(PrefixExtent::new(NonZeroUsize::MIN)),
            [] => Err(TinyError::Incomplete),
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
    pub layout StaticPadded {
        tail @ 4: BeU16;
        align(4) @ 3;
        padding(2) @ 2;
        head @ 1: U8;
    }

    pub layout BoundaryOne {
        tail @ 3: U8;
        align(1) @ 2;
        head @ 1: U8;
    }

    pub layout DynamicPadded {
        tail @ 5: BeU16;
        align(4) @ 4;
        padding(1) @ 3;
        value @ 2: variable(TinyPrefix);
        head @ 1: U8;
    }
}

#[test]
fn fixed_padding_and_alignment_preserve_opaque_bytes_and_static_width() {
    assert_eq!(StaticPadded::WIDTH, 6);
    let input = [7, 0xaa, 0xbb, 0xcc, 0x12, 0x34, 0x99];
    let (view, suffix) = StaticPadded::view(&input)
        .with_remainder()
        .expect("layout should parse");
    assert_eq!(view.as_bytes(), &input[..6]);
    assert_eq!(suffix, &[0x99]);
    assert_eq!(view.head(), 7);
    assert_eq!(view.tail(), 0x1234);

    assert_eq!(BoundaryOne::WIDTH, 2);
    let boundary_one = BoundaryOne::view(&[1, 2])
        .without_trailing()
        .expect("boundary one is a no-op");
    assert_eq!(boundary_one.head(), 1);
    assert_eq!(boundary_one.tail(), 2);
}

#[test]
fn dynamic_alignment_uses_the_represented_offset_and_preserves_exact_boundaries() {
    let canonical = [7, 42, 0xaa, 0xbb, 0x12, 0x34, 0x99];
    let (view, suffix) = DynamicPadded::view(&canonical)
        .with_remainder()
        .expect("layout should parse");
    assert_eq!(view.as_bytes(), &canonical[..6]);
    assert_eq!(suffix, &[0x99]);
    assert_eq!(view.head(), 7);
    assert_eq!(view.value_raw(), &[42]);
    assert_eq!(view.value(), 41);
    assert_eq!(view.tail(), 0x1234);

    let noncanonical = [7, 0, 41, 0xaa, 0x12, 0x34];
    let view = DynamicPadded::view(&noncanonical)
        .without_trailing()
        .expect("layout should parse");
    assert_eq!(view.as_bytes(), &noncanonical);
    assert_eq!(view.value_raw(), &[0, 41]);
    assert_eq!(view.value(), 41);
    assert_eq!(view.tail(), 0x1234);
}

#[test]
fn dynamic_padding_and_alignment_shortage_errors_identify_physical_positions() {
    assert!(matches!(
        DynamicPadded::view(&[7, 42]).with_remainder(),
        Err(DynamicPaddedError::InputTooShort {
            position: 3,
            expected: 1,
            available: 0,
        })
    ));
    assert!(matches!(
        DynamicPadded::view(&[7, 42, 0xaa]).with_remainder(),
        Err(DynamicPaddedError::InputTooShort {
            position: 4,
            expected: 1,
            available: 0,
        })
    ));
}
