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
