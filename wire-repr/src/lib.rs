#![no_std]
#![deny(missing_docs, unsafe_code)]
#![doc = include_str!("lib.md")]

extern crate self as wire_repr;

pub mod output;
mod schema;

/// Zero-sized schema marker types for variable physical representations.
pub mod wire {
    /// A variable-length raw byte field controlled by `bytes = path` or terminal `rest`.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct Bytes;
}
#[doc(hidden)]
pub mod __private {
    pub use crate::schema::{
        ConstantMismatch, Frame, InvalidFrameExtent, IsSet, LayoutError, NeedMore,
        ScalarBuildConversionError, ScalarConversionError, Set, TrailingBytes, Unset,
        checked_align, checked_optional_sum,
    };
    pub use thiserror::Error as ThisError;
}

pub use output::{ChildWriter, GrowthRequest, Output, OutputError, WriteError, Writer, Written};
#[doc(inline)]
pub use wire_repr_macros::{WireBuilder, WireView, validator};

pub use schema::{
    ConstantMismatch, Frame, InvalidFrameExtent, LayoutError, NeedMore, ScalarBuildConversionError,
    ScalarConversionError, TrailingBytes, WireBuilder, WireView, WireWrite,
};
