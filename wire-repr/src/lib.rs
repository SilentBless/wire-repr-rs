#![no_std]
#![deny(missing_docs, unsafe_code)]
#![doc = include_str!("lib.md")]

extern crate self as wire_repr;

pub mod output;
mod schema;

#[doc(hidden)]
pub mod __private {
    pub use crate::schema::{
        ConstantMismatch, Frame, InvalidFrameExtent, NeedMore, ScalarBuildConversionError,
        ScalarConversionError, Set, TrailingBytes, Unset,
    };
    pub use thiserror::Error as ThisError;
}

pub use output::{ChildWriter, GrowthRequest, Output, OutputError, WriteError, Writer, Written};
#[doc(inline)]
pub use wire_repr_macros::{WireBuilder, WireView, validator};

pub use schema::{
    ConstantMismatch, Frame, InvalidFrameExtent, NeedMore, ScalarBuildConversionError,
    ScalarConversionError, TrailingBytes, WireBuilder, WireView, WireWrite,
};
