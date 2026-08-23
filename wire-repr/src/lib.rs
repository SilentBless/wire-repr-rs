#![no_std]
#![deny(missing_docs, unsafe_code)]
#![doc = include_str!("lib.md")]

extern crate self as wire_repr;

/// Codec contracts and built-in codecs.
pub mod codec;

/// Helpers for computed wire fields.
pub mod computation;

mod selection;
mod wire;

/// Implementation details used by `#[derive(Wire)]`.
#[doc(hidden)]
pub mod __private {
    pub use crate::codec::fixed::OwnedBytes;
    pub use crate::codec::{BorrowedSource, ByteChain, EmptySource};
    #[cfg(feature = "bytes")]
    pub use bytes::{Bytes, BytesMut};
}

#[doc(inline)]
pub use wire_repr_macros::Wire;

pub use codec::{
    BeI16, BeI32, BeI64, BeI128, BeU16, BeU24, BeU32, BeU64, BeU128, ByteBytes, ByteChunks,
    ByteRange, ByteSegment, ByteSegmentBytes, ByteSink, ByteSource, ByteSourceCursor, Bytes,
    ExactWidthError, FixedCodec, I8, LeI16, LeI32, LeI64, LeI128, LeU16, LeU24, LeU32, LeU64,
    LeU128, OutputTooShortError, PrefixCodec, PrefixExtent, PreparedLayout, RangeSegments, U8,
    U24RangeError,
};

#[doc(hidden)]
pub use selection::{
    ByteSelection, DirectFieldProjection, DirectFieldSelection, ExcludedBytes, FieldProjection,
    FieldSelection, FieldUnion, IncludedBytes, MarkerScope, NestedField, RootScope,
    SelectedSegments, Through, Translated,
};

pub use wire::{
    BuildIntoError, FixedValidatedViewSequenceError, FixedViewIterator, FixedViewSequenceError,
    ValidatedViewCursor, ValidatedViewRequest, ViewCursor, ViewCursorError, ViewRequest,
    WireEncode, WireView, WireViewType, WireViewValidation, Written,
};
