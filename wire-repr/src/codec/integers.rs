//! Built-in fixed-width integer codecs.

use core::{convert::Infallible, fmt};

use super::FixedCodec;

/// Error returned when a `u32` does not fit in an unsigned 24-bit integer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct U24RangeError {
    value: u32,
}

impl U24RangeError {
    /// Creates an error for an unrepresentable value.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self { value }
    }

    /// Returns the unrepresentable value.
    #[must_use]
    pub const fn value(&self) -> u32 {
        self.value
    }
}

impl fmt::Display for U24RangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} does not fit in an unsigned 24-bit integer",
            self.value
        )
    }
}

impl core::error::Error for U24RangeError {}

macro_rules! fixed_integer_codec {
    ($name:ident, $value:ty, $width:literal, $from:ident, $to:ident) => {
        impl FixedCodec for $name {
            type Value<'wire>
                = $value
            where
                Self: 'wire;
            type EncodeError = Infallible;
            type Plan<'value>
                = [u8; $width]
            where
                Self: 'value;

            const WIDTH: usize = $width;

            #[inline]
            fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
                let mut encoded = [0_u8; $width];
                encoded.copy_from_slice(bytes);
                <$value>::$from(encoded)
            }

            #[inline]
            fn plan<'value>(
                value: Self::Value<'value>,
            ) -> Result<Self::Plan<'value>, Self::EncodeError> {
                Ok(value.$to())
            }
        }
    };
}

/// One-byte unsigned integer codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct U8;
fixed_integer_codec!(U8, u8, 1, from_ne_bytes, to_ne_bytes);

/// One-byte signed integer codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct I8;
fixed_integer_codec!(I8, i8, 1, from_ne_bytes, to_ne_bytes);

/// Big-endian `u16` codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BeU16;
fixed_integer_codec!(BeU16, u16, 2, from_be_bytes, to_be_bytes);

/// Little-endian `u16` codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeU16;
fixed_integer_codec!(LeU16, u16, 2, from_le_bytes, to_le_bytes);

/// Big-endian `i16` codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BeI16;
fixed_integer_codec!(BeI16, i16, 2, from_be_bytes, to_be_bytes);

/// Little-endian `i16` codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeI16;
fixed_integer_codec!(LeI16, i16, 2, from_le_bytes, to_le_bytes);

/// Big-endian `u32` codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BeU32;
fixed_integer_codec!(BeU32, u32, 4, from_be_bytes, to_be_bytes);

/// Little-endian `u32` codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeU32;
fixed_integer_codec!(LeU32, u32, 4, from_le_bytes, to_le_bytes);

/// Big-endian `i32` codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BeI32;
fixed_integer_codec!(BeI32, i32, 4, from_be_bytes, to_be_bytes);

/// Little-endian `i32` codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeI32;
fixed_integer_codec!(LeI32, i32, 4, from_le_bytes, to_le_bytes);

/// Big-endian `u64` codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BeU64;
fixed_integer_codec!(BeU64, u64, 8, from_be_bytes, to_be_bytes);

/// Little-endian `u64` codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeU64;
fixed_integer_codec!(LeU64, u64, 8, from_le_bytes, to_le_bytes);

/// Big-endian `i64` codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BeI64;
fixed_integer_codec!(BeI64, i64, 8, from_be_bytes, to_be_bytes);

/// Little-endian `i64` codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeI64;
fixed_integer_codec!(LeI64, i64, 8, from_le_bytes, to_le_bytes);

/// Big-endian `u128` codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BeU128;
fixed_integer_codec!(BeU128, u128, 16, from_be_bytes, to_be_bytes);

/// Little-endian `u128` codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeU128;
fixed_integer_codec!(LeU128, u128, 16, from_le_bytes, to_le_bytes);

/// Big-endian `i128` codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BeI128;
fixed_integer_codec!(BeI128, i128, 16, from_be_bytes, to_be_bytes);

/// Little-endian `i128` codec.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeI128;
fixed_integer_codec!(LeI128, i128, 16, from_le_bytes, to_le_bytes);

#[inline]
fn decode_be_u24(bytes: [u8; 3]) -> u32 {
    u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]])
}

#[inline]
fn decode_le_u24(bytes: [u8; 3]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0])
}

#[inline]
fn encode_be_u24(value: u32) -> [u8; 3] {
    let bytes = value.to_be_bytes();
    [bytes[1], bytes[2], bytes[3]]
}

#[inline]
fn encode_le_u24(value: u32) -> [u8; 3] {
    let bytes = value.to_le_bytes();
    [bytes[0], bytes[1], bytes[2]]
}

/// Big-endian unsigned 24-bit integer codec over `u32`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BeU24;

impl FixedCodec for BeU24 {
    type Value<'wire>
        = u32
    where
        Self: 'wire;
    type EncodeError = U24RangeError;
    type Plan<'value>
        = [u8; 3]
    where
        Self: 'value;

    const WIDTH: usize = 3;

    #[inline]
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        let mut encoded = [0_u8; 3];
        encoded.copy_from_slice(bytes);
        decode_be_u24(encoded)
    }

    #[inline]
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        if value > 0x00ff_ffff {
            Err(U24RangeError::new(value))
        } else {
            Ok(encode_be_u24(value))
        }
    }
}

/// Little-endian unsigned 24-bit integer codec over `u32`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeU24;

impl FixedCodec for LeU24 {
    type Value<'wire>
        = u32
    where
        Self: 'wire;
    type EncodeError = U24RangeError;
    type Plan<'value>
        = [u8; 3]
    where
        Self: 'value;

    const WIDTH: usize = 3;

    #[inline]
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        let mut encoded = [0_u8; 3];
        encoded.copy_from_slice(bytes);
        decode_le_u24(encoded)
    }

    #[inline]
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        if value > 0x00ff_ffff {
            Err(U24RangeError::new(value))
        } else {
            Ok(encode_le_u24(value))
        }
    }
}
