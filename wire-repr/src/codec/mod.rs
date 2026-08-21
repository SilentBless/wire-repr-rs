#![doc = include_str!("mod.md")]

pub(crate) mod fixed;
mod integers;
mod plan;
mod prefix;
mod range_source;

pub use fixed::{Bytes, ExactWidthError, FixedCodec};
pub use integers::{
    BeI16, BeI32, BeI64, BeI128, BeU16, BeU24, BeU32, BeU64, BeU128, I8, LeI16, LeI32, LeI64,
    LeI128, LeU16, LeU24, LeU32, LeU64, LeU128, U8, U24RangeError,
};
pub use plan::{EncodePlan, OutputTooShortError, PreparedLayout};
pub use prefix::{PrefixCodec, PrefixExtent};
pub use range_source::RangeSource;
