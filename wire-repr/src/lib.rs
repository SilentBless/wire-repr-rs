#![no_std]
#![deny(missing_docs, unsafe_code)]
#![doc = include_str!("lib.md")]

extern crate self as wire_repr;

pub mod output;
mod recursive;
mod schema;

/// Zero-sized schema marker types for variable physical representations.
pub mod wire {
    /// A variable-length raw byte field controlled by `bytes = path` or terminal `rest`.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct Bytes;

    /// A zero-sized runtime array marker controlled by `counted_by = path`.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    pub struct Array<T>(core::marker::PhantomData<fn() -> T>);
}
#[doc(hidden)]
pub mod __private {
    pub use crate::recursive::{
        FlattenRecursiveError, RecursiveArrayView, RecursiveBody, RecursiveChildren,
        RecursiveDepth, RecursiveError, RecursiveFrame, RecursiveGeometry,
        RecursiveGeometryBuilder, RecursiveSlot, flatten_recursive_array_error,
        frame_recursive_array_extent,
    };
    pub use crate::schema::{
        ArrayError, ArrayItem, ArrayView, ConstantMismatch, FieldExpr, Frame, InvalidFrameExtent,
        IsSet, LayoutError, LeadingWire, NeedMore, ScalarBuildConversionError,
        ScalarConversionError, Set, TrailingBytes, Unset, WireFields, WireSelect, checked_align,
        checked_optional_equal, checked_optional_sum, frame_array_extent,
    };
    pub use thiserror::Error as ThisError;
}

pub use output::{
    ArrayWriter, ChildWriter, GrowthRequest, Output, OutputError, WriteError, Writer, Written,
};
pub use recursive::DepthExceeded;
pub use schema::{
    ArrayError, ArrayItem, ArrayIter, ArrayView, ByteSelection, ConstantMismatch, Cursor,
    ExactWire, FieldPath, FieldUnion, FixedViews, Frame, InvalidFrameExtent, LayoutError, NeedMore,
    NextView, ScalarBuildConversionError, ScalarConversionError, Selection, SelectionBytes,
    SelectionChunks, SequenceError, TrailingBytes, VariableViews, WireBuilder, WireBytes, WireView,
    WireWrite, select,
};
#[doc(inline)]
pub use wire_repr_macros::{WireBuilder, WireView, computed, validator};
