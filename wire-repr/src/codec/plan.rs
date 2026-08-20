/// A completed, infallible encoding operation.
///
/// Implementations perform all fallible work while they are created. `write_into`
/// therefore only copies already-prepared bytes into an exactly-sized output slice.
pub trait EncodePlan {
    /// Returns the exact number of bytes written by [`Self::write_into`].
    #[must_use]
    fn encoded_len(&self) -> usize;

    /// Writes this plan into `output`.
    ///
    /// `output` must have length [`Self::encoded_len`]. Passing another length is a
    /// contract violation and may panic; implementations must not silently succeed
    /// without writing the complete encoding.
    fn write_into(&self, output: &mut [u8]);
}

/// A prepared layout encoding that can be committed into an output buffer.
///
/// Preparation performs every fallible codec operation. Implementations only check
/// output capacity and copy already-prepared encodings when committed.
pub trait PreparedLayout {
    /// The mutable view returned over the committed layout bytes.
    type ViewMut<'output>;

    /// Returns the exact number of output bytes required for this layout.
    #[must_use]
    fn encoded_len(&self) -> usize;

    /// Commits this prepared layout into the leading output bytes.
    ///
    /// Extra output bytes are returned as a disjoint suffix. A short output is left
    /// unchanged.
    fn commit_into<'output>(
        self,
        output: &'output mut [u8],
    ) -> Result<(Self::ViewMut<'output>, &'output mut [u8]), OutputTooShortError>;
}

/// Reports that an output buffer cannot contain a prepared layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputTooShortError {
    /// The exact number of bytes required by the prepared layout.
    pub required: usize,
    /// The number of bytes available in the supplied output buffer.
    pub available: usize,
}

impl core::fmt::Display for OutputTooShortError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "output too short: need {} bytes, got {}",
            self.required, self.available
        )
    }
}

impl core::error::Error for OutputTooShortError {}

impl<const N: usize> EncodePlan for [u8; N] {
    #[inline]
    fn encoded_len(&self) -> usize {
        N
    }

    #[inline]
    fn write_into(&self, output: &mut [u8]) {
        output.copy_from_slice(self);
    }
}

impl EncodePlan for &[u8] {
    #[inline]
    fn encoded_len(&self) -> usize {
        self.len()
    }

    #[inline]
    fn write_into(&self, output: &mut [u8]) {
        output.copy_from_slice(self);
    }
}
