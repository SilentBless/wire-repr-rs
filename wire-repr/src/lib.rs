#![no_std]
#![deny(missing_docs, unsafe_code)]
#![doc = include_str!("lib.md")]

extern crate self as wire_repr;

/// Codec contracts and built-in codecs.
pub mod codec;

/// Implementation details used by code generated from [`wire_repr!`].
#[doc(hidden)]
pub mod __private {
    pub use crate::codec::fixed::OwnedBytes;
}

#[doc(inline)]
pub use wire_repr_macros::wire_repr;

pub use codec::{
    BeI16, BeI32, BeI64, BeI128, BeU16, BeU24, BeU32, BeU64, BeU128, Bytes, Discriminant,
    EncodePlan, ExactWidthError, FixedCodec, I8, LeI16, LeI32, LeI64, LeI128, LeU16, LeU24, LeU32,
    LeU64, LeU128, OutputTooShortError, PrefixCodec, PrefixExtent, PreparedLayout, RangeSource, U8,
    U24RangeError, UnknownBody,
};
