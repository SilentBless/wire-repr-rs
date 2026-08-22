/// Receives ordered chunks of an encoded byte sequence.
///
/// Implementations can write chunks directly to their destination. [`Self::fill`] represents
/// repeated bytes without requiring an intermediate allocation or a large stack buffer.
pub trait ByteSink {
    /// Writes one ordered chunk.
    fn write(&mut self, bytes: &[u8]);

    /// Writes `len` copies of `byte` after all preceding chunks.
    fn fill(&mut self, byte: u8, len: usize);
}

/// One physical span of bytes from a [`ByteSourceCursor`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ByteSegment<'bytes> {
    /// Bytes borrowed directly from their source.
    Bytes(&'bytes [u8]),
    /// Remaining encoded bytes represented without materializing a buffer.
    Rest {
        /// The repeated byte value.
        byte: u8,
        /// The number of remaining bytes.
        len: usize,
    },
}

impl<'bytes> ByteSegment<'bytes> {
    /// Returns the number of bytes in this segment.
    #[must_use]
    pub const fn len(self) -> usize {
        match self {
            Self::Bytes(bytes) => bytes.len(),
            Self::Rest { len, .. } => len,
        }
    }

    /// Returns whether this segment contains no bytes.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// Iterates over this segment's bytes in order.
    #[must_use]
    pub fn bytes(self) -> ByteSegmentBytes<'bytes> {
        match self {
            Self::Bytes(bytes) => ByteSegmentBytes::Bytes(bytes.iter()),
            Self::Rest { byte, len } => ByteSegmentBytes::Rest {
                byte,
                remaining: len,
            },
        }
    }

    fn split_at(self, len: usize) -> (Self, Option<Self>) {
        match self {
            Self::Bytes(bytes) => {
                let (head, tail) = bytes.split_at(len);
                (
                    Self::Bytes(head),
                    (!tail.is_empty()).then_some(Self::Bytes(tail)),
                )
            }
            Self::Rest { byte, len: total } => (
                Self::Rest { byte, len },
                (len != total).then_some(Self::Rest {
                    byte,
                    len: total - len,
                }),
            ),
        }
    }

    pub(crate) fn subsegment(self, start: usize, len: usize) -> Self {
        match self {
            Self::Bytes(bytes) => Self::Bytes(&bytes[start..start + len]),
            Self::Rest { byte, .. } => Self::Rest { byte, len },
        }
    }
}

impl ByteSource for ByteSegment<'_> {
    #[inline(always)]
    fn byte_len(&self) -> usize {
        self.len()
    }

    #[inline(always)]
    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        match *self {
            Self::Bytes(bytes) => sink.write(bytes),
            Self::Rest { byte, len } => sink.fill(byte, len),
        }
    }
}

impl ByteSourceCursor for ByteSegment<'_> {
    type Segments<'source>
        = core::iter::Once<ByteSegment<'source>>
    where
        Self: 'source;

    #[inline(always)]
    fn segments(&self) -> Self::Segments<'_> {
        core::iter::once(*self)
    }
}

/// Byte iterator returned by [`ByteSegment::bytes`].
pub enum ByteSegmentBytes<'bytes> {
    /// Iterates borrowed bytes.
    Bytes(core::slice::Iter<'bytes, u8>),
    /// Iterates the remaining repeated bytes.
    Rest {
        /// The repeated byte value.
        byte: u8,
        /// The number of bytes not yet yielded.
        remaining: usize,
    },
}

impl Iterator for ByteSegmentBytes<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Bytes(bytes) => bytes.next().copied(),
            Self::Rest { byte, remaining } => {
                if *remaining == 0 {
                    None
                } else {
                    *remaining -= 1;
                    Some(*byte)
                }
            }
        }
    }
}

/// A static, zero-copy cursor over a [`ByteSource`]'s physical segments.
pub trait ByteSourceCursor: ByteSource {
    /// The source's physical-segment iterator.
    type Segments<'source>: Iterator<Item = ByteSegment<'source>>
    where
        Self: 'source;

    /// Returns the source's natural physical byte spans.
    #[must_use]
    fn segments(&self) -> Self::Segments<'_>;

    /// Returns ordered byte spans no longer than `max_len`.
    ///
    /// Panics when `max_len` is zero, matching [`slice::chunks`].
    #[must_use]
    fn chunks(&self, max_len: usize) -> ByteChunks<'_, Self::Segments<'_>> {
        assert_ne!(max_len, 0, "chunk size must be non-zero");
        ByteChunks {
            segments: self.segments(),
            max_len,
            remainder: None,
        }
    }

    /// Flattens the source's physical spans into ordered bytes.
    #[must_use]
    fn bytes(&self) -> ByteBytes<'_, Self::Segments<'_>> {
        ByteBytes {
            segments: self.segments(),
            current: None,
        }
    }

    /// Returns a zero-copy source over one logical byte range.
    ///
    /// Panics when the range is reversed, overflows, or lies outside this source.
    #[must_use]
    fn range<R: core::ops::RangeBounds<usize>>(&self, range: R) -> ByteRange<'_, Self>
    where
        Self: Sized,
    {
        ByteRange::new(self, range)
    }
}

/// An iterator that splits physical spans into bounded byte chunks.
pub struct ByteChunks<'source, I> {
    segments: I,
    max_len: usize,
    remainder: Option<ByteSegment<'source>>,
}

impl<'source, I: Iterator<Item = ByteSegment<'source>>> Iterator for ByteChunks<'source, I> {
    type Item = ByteSegment<'source>;

    fn next(&mut self) -> Option<Self::Item> {
        let segment = self.remainder.take().or_else(|| self.segments.next())?;
        if segment.len() <= self.max_len {
            Some(segment)
        } else {
            let (chunk, remainder) = segment.split_at(self.max_len);
            self.remainder = remainder;
            Some(chunk)
        }
    }
}

/// An iterator that flattens physical byte spans.
pub struct ByteBytes<'source, I> {
    segments: I,
    current: Option<ByteSegmentBytes<'source>>,
}

impl<'source, I: Iterator<Item = ByteSegment<'source>>> Iterator for ByteBytes<'source, I> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(bytes) = &mut self.current
                && let Some(byte) = bytes.next()
            {
                return Some(byte);
            }
            self.current = self.segments.next().map(ByteSegment::bytes);
            self.current.as_ref()?;
        }
    }
}

/// A zero-copy logical range of another byte source.
pub struct ByteRange<'source, T: ?Sized> {
    source: &'source T,
    start: usize,
    end: usize,
}

impl<'source, T: ByteSourceCursor + ?Sized> ByteRange<'source, T> {
    fn new<R: core::ops::RangeBounds<usize>>(source: &'source T, range: R) -> Self {
        use core::ops::Bound;

        let start = match range.start_bound() {
            Bound::Included(&start) => start,
            Bound::Excluded(&start) => start.checked_add(1).expect("byte range start overflow"),
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&end) => end.checked_add(1).expect("byte range end overflow"),
            Bound::Excluded(&end) => end,
            Bound::Unbounded => source.byte_len(),
        };
        assert!(start <= end, "byte range starts after its end");
        assert!(end <= source.byte_len(), "byte range is outside the source");
        Self { source, start, end }
    }
}

impl<T: ByteSourceCursor + ?Sized> ByteSource for ByteRange<'_, T> {
    #[inline]
    fn byte_len(&self) -> usize {
        self.end - self.start
    }

    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        for segment in self.segments() {
            match segment {
                ByteSegment::Bytes(bytes) => sink.write(bytes),
                ByteSegment::Rest { byte, len } => sink.fill(byte, len),
            }
        }
    }
}

impl<T: ByteSourceCursor + ?Sized> ByteSourceCursor for ByteRange<'_, T> {
    type Segments<'source>
        = RangeSegments<T::Segments<'source>>
    where
        Self: 'source;

    fn segments(&self) -> Self::Segments<'_> {
        RangeSegments {
            segments: self.source.segments(),
            source_position: 0,
            start: self.start,
            end: self.end,
        }
    }
}

/// A cursor over a logical range of another segment iterator.
pub struct RangeSegments<I> {
    segments: I,
    source_position: usize,
    start: usize,
    end: usize,
}

impl<'source, I: Iterator<Item = ByteSegment<'source>>> Iterator for RangeSegments<I> {
    type Item = ByteSegment<'source>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.source_position < self.end {
            let segment = self.segments.next()?;
            let segment_start = self.source_position;
            let segment_end = segment_start
                .checked_add(segment.len())
                .expect("ByteSource segment length overflow");
            self.source_position = segment_end;
            let start = segment_start.max(self.start);
            let end = segment_end.min(self.end);
            if start < end {
                return Some(segment.subsegment(start - segment_start, end - start));
            }
        }
        None
    }
}

/// A one-segment cursor over borrowed bytes.
pub struct SingleSegment<'bytes>(Option<&'bytes [u8]>);

impl<'bytes> Iterator for SingleSegment<'bytes> {
    type Item = ByteSegment<'bytes>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.take().map(ByteSegment::Bytes)
    }
}

/// A cursor that concatenates two segment iterators.
pub struct ChainSegments<L, R> {
    left: L,
    right: R,
}

impl<'bytes, L: Iterator<Item = ByteSegment<'bytes>>, R: Iterator<Item = ByteSegment<'bytes>>>
    Iterator for ChainSegments<L, R>
{
    type Item = ByteSegment<'bytes>;

    fn next(&mut self) -> Option<Self::Item> {
        self.left.next().or_else(|| self.right.next())
    }
}

/// A completed, infallible source of encoded bytes.
///
/// Implementations perform all fallible work while they are created. [`Self::emit_to`] emits
/// exactly [`Self::byte_len`] bytes in order; violating that contract causes
/// [`Self::write_into`] to panic.
pub trait ByteSource {
    /// Returns the exact number of bytes emitted by [`Self::emit_to`].
    #[must_use]
    fn byte_len(&self) -> usize;

    /// Emits the prepared bytes to `sink` in order.
    fn emit_to<S: ByteSink>(&self, sink: &mut S);

    /// Writes this source into an exactly-sized output slice.
    ///
    /// `output` must have length [`Self::byte_len`]. Passing another length is a contract
    /// violation and panics. This method also panics when an implementation under- or
    /// over-emits its declared byte length.
    #[inline(always)]
    fn write_into(&self, output: &mut [u8]) {
        assert_eq!(
            output.len(),
            self.byte_len(),
            "ByteSource output length mismatch"
        );
        let mut sink = SliceSink { output, written: 0 };
        self.emit_to(&mut sink);
        assert_eq!(
            sink.written,
            sink.output.len(),
            "ByteSource emitted wrong length"
        );
    }
}

struct SliceSink<'output> {
    output: &'output mut [u8],
    written: usize,
}

impl ByteSink for SliceSink<'_> {
    #[inline(always)]
    fn write(&mut self, bytes: &[u8]) {
        let end = self
            .written
            .checked_add(bytes.len())
            .expect("ByteSource emission length overflow");
        let output = self
            .output
            .get_mut(self.written..end)
            .expect("ByteSource emitted too many bytes");
        output.copy_from_slice(bytes);
        self.written = end;
    }

    #[inline(always)]
    fn fill(&mut self, byte: u8, len: usize) {
        let end = self
            .written
            .checked_add(len)
            .expect("ByteSource emission length overflow");
        let output = self
            .output
            .get_mut(self.written..end)
            .expect("ByteSource emitted too many bytes");
        output.fill(byte);
        self.written = end;
    }
}

/// A prepared wire encoding that can be committed into an output buffer.
///
/// Preparation performs every fallible codec operation. Implementations only check
/// output capacity and emit already-prepared encodings when committed.
pub trait PreparedLayout: ByteSourceCursor {
    /// The committed output wrapper.
    type Written<'output>;

    /// Returns the exact number of output bytes required for this encoding.
    #[must_use]
    fn encoded_len(&self) -> usize {
        self.byte_len()
    }

    /// Commits this prepared encoding into the leading output bytes.
    ///
    /// Extra output bytes are returned as a disjoint suffix. A short output is left
    /// unchanged.
    fn commit_into<'output>(
        self,
        output: &'output mut [u8],
    ) -> Result<(Self::Written<'output>, &'output mut [u8]), OutputTooShortError>;
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

impl<const N: usize> ByteSource for [u8; N] {
    #[inline]
    fn byte_len(&self) -> usize {
        N
    }

    #[inline]
    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        sink.write(self);
    }
}

impl<const N: usize> ByteSourceCursor for [u8; N] {
    type Segments<'source>
        = SingleSegment<'source>
    where
        Self: 'source;

    #[inline]
    fn segments(&self) -> Self::Segments<'_> {
        SingleSegment(Some(self))
    }
}

impl ByteSource for &[u8] {
    #[inline]
    fn byte_len(&self) -> usize {
        self.len()
    }

    #[inline]
    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        sink.write(self);
    }
}

impl ByteSourceCursor for &[u8] {
    type Segments<'source>
        = SingleSegment<'source>
    where
        Self: 'source;

    #[inline]
    fn segments(&self) -> Self::Segments<'_> {
        SingleSegment(Some(self))
    }
}

/// A macro-support reference to an existing byte source.
#[doc(hidden)]
pub struct BorrowedSource<'source, T: ?Sized> {
    source: &'source T,
}

impl<'source, T: ?Sized> BorrowedSource<'source, T> {
    /// Wraps `source` without changing its representation.
    #[must_use]
    pub const fn new(source: &'source T) -> Self {
        Self { source }
    }
}

impl<T: ByteSource + ?Sized> ByteSource for BorrowedSource<'_, T> {
    #[inline(always)]
    fn byte_len(&self) -> usize {
        self.source.byte_len()
    }

    #[inline(always)]
    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        self.source.emit_to(sink);
    }
}

impl<T: ByteSourceCursor + ?Sized> ByteSourceCursor for BorrowedSource<'_, T> {
    type Segments<'source>
        = T::Segments<'source>
    where
        Self: 'source;

    #[inline(always)]
    fn segments(&self) -> Self::Segments<'_> {
        self.source.segments()
    }
}

/// A macro-support concatenation of two completed byte sources.
#[doc(hidden)]
pub struct ByteChain<L, R> {
    left: L,
    right: R,
}

impl<L, R> ByteChain<L, R> {
    /// Concatenates `left` and `right` in physical order.
    #[must_use]
    pub const fn new(left: L, right: R) -> Self {
        Self { left, right }
    }
}

impl<L: ByteSource, R: ByteSource> ByteSource for ByteChain<L, R> {
    #[inline(always)]
    fn byte_len(&self) -> usize {
        self.left
            .byte_len()
            .checked_add(self.right.byte_len())
            .expect("computed byte source length overflow")
    }

    #[inline(always)]
    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        self.left.emit_to(sink);
        self.right.emit_to(sink);
    }
}

impl<L: ByteSourceCursor, R: ByteSourceCursor> ByteSourceCursor for ByteChain<L, R> {
    type Segments<'source>
        = ChainSegments<L::Segments<'source>, R::Segments<'source>>
    where
        Self: 'source;

    #[inline(always)]
    fn segments(&self) -> Self::Segments<'_> {
        ChainSegments {
            left: self.left.segments(),
            right: self.right.segments(),
        }
    }
}

/// An empty macro-support byte source.
#[doc(hidden)]
pub struct EmptySource;

impl ByteSource for EmptySource {
    #[inline(always)]
    fn byte_len(&self) -> usize {
        0
    }

    #[inline(always)]
    fn emit_to<S: ByteSink>(&self, _sink: &mut S) {}
}

impl ByteSourceCursor for EmptySource {
    type Segments<'source>
        = core::iter::Empty<ByteSegment<'source>>
    where
        Self: 'source;

    #[inline(always)]
    fn segments(&self) -> Self::Segments<'_> {
        core::iter::empty()
    }
}
