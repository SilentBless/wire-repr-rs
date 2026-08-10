//! Fixed-width codec contracts.

use super::EncodePlan;

/// A codec whose encoded representation always has one fixed width.
///
/// [`Self::WIDTH`] must be nonzero. Every successful [`Self::plan`] must report that
/// exact encoded length. For every such plan, [`EncodePlan::write_into`] called with
/// an output slice of exactly [`Self::WIDTH`] bytes must write a complete representation
/// whose decoding recovers the same semantic value supplied to [`Self::plan`]. Decoding
/// is total for every exact-width byte pattern. A codec that violates these requirements
/// is contract-invalid.
///
/// When a layout builder derives a region length through
/// `Self::Value<'static>: TryFrom<usize>`, the complete conversion and codec round trip
/// must preserve that length: converting the decoded planned representation back to
/// `usize` must produce the original region length.
///
/// [`Self::plan`] completes all fallible encoding work before a caller mutates an output
/// buffer. Layout parsing establishes exact-width bounds before calling [`Self::decode`].
pub trait FixedCodec {
    /// Semantic value represented by an exact-width wire representation.
    type Value<'wire>
    where
        Self: 'wire;

    /// Error returned while preparing an encoded value.
    type EncodeError: core::fmt::Debug;

    /// Prepared fixed-width encoded bytes.
    type Plan<'value>: EncodePlan
    where
        Self: 'value;

    /// Number of bytes in every encoded representation.
    const WIDTH: usize;

    /// Decodes an exact-width encoded representation.
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire>;

    /// Prepares the complete encoded representation without mutating a caller buffer.
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError>;
}

/// Error returned when an exact-width byte value has the wrong length.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExactWidthError {
    expected: usize,
    actual: usize,
}

impl ExactWidthError {
    /// Creates an error for an exact-width mismatch.
    #[must_use]
    pub const fn new(expected: usize, actual: usize) -> Self {
        Self { expected, actual }
    }

    /// Returns the required byte length.
    #[must_use]
    pub const fn expected(&self) -> usize {
        self.expected
    }

    /// Returns the supplied byte length.
    #[must_use]
    pub const fn actual(&self) -> usize {
        self.actual
    }
}

impl core::fmt::Display for ExactWidthError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "fixed codec expected {} bytes, got {}",
            self.expected, self.actual
        )
    }
}

impl core::error::Error for ExactWidthError {}

/// A borrowed fixed-width span of wire bytes with no content interpretation.
///
/// `N` must be nonzero. Using `Bytes<0>` as a [`FixedCodec`] fails during constant
/// evaluation rather than exposing a codec that violates [`FixedCodec::WIDTH`].
///
/// ```compile_fail
/// use wire_repr::{Bytes, FixedCodec};
///
/// let _ = <Bytes<0> as FixedCodec>::WIDTH;
/// ```
///
/// `Bytes<N>` decodes to the exact borrowed wire slice and plans an equally borrowed input
/// slice for copying at write time. It does not validate magic values, reserved bytes, or
/// any other domain semantics; consumers own those policies.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Bytes<const N: usize>;

impl<const N: usize> FixedCodec for Bytes<N> {
    type Value<'wire>
        = &'wire [u8]
    where
        Self: 'wire;
    type EncodeError = ExactWidthError;
    type Plan<'value>
        = &'value [u8]
    where
        Self: 'value;

    const WIDTH: usize = {
        assert!(N != 0, "Bytes<N> requires a nonzero width");
        N
    };

    #[inline]
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire> {
        const { assert!(N != 0, "Bytes<N> requires a nonzero width") };
        bytes
    }

    #[inline]
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError> {
        const { assert!(N != 0, "Bytes<N> requires a nonzero width") };
        if value.len() == N {
            Ok(value)
        } else {
            Err(ExactWidthError::new(N, value.len()))
        }
    }
}
