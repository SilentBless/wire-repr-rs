//! Prefix codec contract.

use core::num::NonZeroUsize;

use super::EncodePlan;

/// The nonzero extent occupied by a validated encoded prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PrefixExtent {
    encoded_len: NonZeroUsize,
}

impl PrefixExtent {
    /// Creates an extent from a nonzero encoded length.
    #[must_use]
    pub const fn new(encoded_len: NonZeroUsize) -> Self {
        Self { encoded_len }
    }

    /// Returns the number of bytes occupied by the encoded prefix.
    #[must_use]
    pub const fn encoded_len(self) -> NonZeroUsize {
        self.encoded_len
    }

    /// Splits input into its encoded prefix and remaining suffix.
    #[inline]
    #[must_use]
    pub fn split_input<'input>(&self, input: &'input [u8]) -> Option<(&'input [u8], &'input [u8])> {
        let encoded_len = self.encoded_len.get();
        if encoded_len > input.len() {
            None
        } else {
            Some(input.split_at(encoded_len))
        }
    }
}

/// A codec whose encoded representation occupies a variable-length prefix.
///
/// [`Self::validate_prefix`] performs structural validation and discovers the exact
/// nonzero extent from available input. A successful extent must not exceed the
/// supplied input. Callers must enforce that implementor law with
/// [`PrefixExtent::split_input`] or an equivalent check before slicing, because custom
/// implementations can violate it.
///
/// [`Self::decode`] receives exactly the encoded bytes for which validation succeeded
/// and whose length equals the reported extent. It decodes semantically without
/// rediscovering a suffix. Calling it on other bytes is a contract violation and may
/// panic. Legal noncanonical input remains the caller's exact bytes; canonicality
/// belongs to [`Self::plan`]. Every successful plan must report a nonzero encoded
/// length and write a complete canonical representation for which
/// [`Self::validate_prefix`] returns that same extent. Decoding those bytes must recover
/// the same semantic value supplied to [`Self::plan`]. A codec that violates these
/// requirements is contract-invalid. All fallible planning completes before a caller
/// buffer is mutated.
///
/// When a layout builder derives a region length through
/// `Self::Value<'static>: TryFrom<usize>`, the complete conversion and codec round trip
/// must preserve that length: converting the decoded planned representation back to
/// `usize` must produce the original region length.
pub trait PrefixCodec {
    /// Semantic value represented by the codec, which may borrow decode input.
    type Value<'wire>
    where
        Self: 'wire;

    /// Error returned while structurally validating a prefix.
    type DecodeError: core::fmt::Debug;

    /// Error returned while preparing an encoded value.
    type EncodeError: core::fmt::Debug;

    /// Prepared canonical encoded bytes, which may borrow the input value.
    type Plan<'value>: EncodePlan
    where
        Self: 'value;

    /// Structurally validates a prefix and reports its exact encoded extent.
    fn validate_prefix(bytes: &[u8]) -> Result<PrefixExtent, Self::DecodeError>;

    /// Decodes exact bytes which have already been successfully prefix-validated.
    ///
    /// `bytes` must be the encoded span selected by the returned [`PrefixExtent`], not
    /// the input that may also contain a suffix. Calling this with other bytes is a
    /// contract violation and may panic.
    fn decode<'wire>(bytes: &'wire [u8]) -> Self::Value<'wire>;

    /// Prepares the complete canonical encoding without mutating a caller buffer.
    fn plan<'value>(value: Self::Value<'value>) -> Result<Self::Plan<'value>, Self::EncodeError>;
}
