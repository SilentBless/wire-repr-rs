//! Progressive contiguous output adapters used by generated writers.

use core::convert::Infallible;
use core::fmt;
use core::ops::Range;

/// A request to expose enough contiguous bytes for the next generated write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GrowthRequest {
    /// Highest output offset established by the current representation.
    pub high_water_mark: usize,
    /// Minimum output length required to continue.
    pub minimum_len: usize,
    /// Generated growth hint. Implementations may expose more or less, but never less than
    /// `minimum_len` on success.
    pub suggested_len: usize,
}

/// Failure to expose enough bytes for a progressive write.
#[derive(Debug)]
pub enum OutputError<E> {
    /// A fixed output ended before the next write.
    NeedMore {
        /// Attempted write and generated lookahead.
        request: GrowthRequest,
        /// Current fixed output length.
        available: usize,
    },
    /// A bounded output would have to exceed its configured limit.
    Limit {
        /// Attempted write and generated lookahead.
        request: GrowthRequest,
        /// Maximum visible length allowed by the bounded adapter.
        limit: usize,
    },
    /// Caller-provided growth failed.
    Grow(E),
    /// A growth implementation returned success without exposing the required span.
    GrowthRefused {
        /// Attempted write and generated lookahead.
        request: GrowthRequest,
        /// Length exposed after the growth implementation returned success.
        actual_len: usize,
    },
    /// A nested writer attempted to seek before the parent's high-water mark.
    Backwards {
        /// Requested child start offset.
        position: usize,
        /// Parent high-water mark that must not be overwritten.
        written: usize,
    },
    /// A fixed-width nested writer exceeded its assigned region.
    ChildOverflow {
        /// Attempted child end offset.
        end: usize,
        /// Exclusive end of the assigned child region.
        limit: usize,
    },
    /// A fixed-width nested writer stopped before filling its assigned region.
    ChildIncomplete {
        /// Actual child end offset.
        end: usize,
        /// Required exclusive end of the assigned child region.
        limit: usize,
    },
    /// Output length arithmetic overflowed `usize`.
    LengthOverflow,
}

impl<E: fmt::Display> fmt::Display for OutputError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NeedMore { request, available } => write!(
                formatter,
                "output needs {} bytes, {} available",
                request.minimum_len, available
            ),
            Self::Limit { request, limit } => write!(
                formatter,
                "output needs {} bytes, limit is {}",
                request.minimum_len, limit
            ),
            Self::Grow(error) => write!(formatter, "output growth failed: {error}"),
            Self::GrowthRefused {
                request,
                actual_len,
            } => write!(
                formatter,
                "output growth exposed {actual_len} bytes, {} required",
                request.minimum_len
            ),
            Self::Backwards { position, written } => write!(
                formatter,
                "nested output position {position} precedes written prefix {written}"
            ),
            Self::ChildOverflow { end, limit } => {
                write!(
                    formatter,
                    "nested output ended at {end}, region ends at {limit}"
                )
            }
            Self::ChildIncomplete { end, limit } => {
                write!(formatter, "nested output ended at {end}, expected {limit}")
            }
            Self::LengthOverflow => formatter.write_str("output length overflow"),
        }
    }
}

impl<E: core::error::Error + 'static> core::error::Error for OutputError<E> {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Grow(error) => Some(error),
            _ => None,
        }
    }
}

/// A schema or output failure while writing progressively.
#[derive(Debug)]
pub enum WriteError<S, G> {
    /// Schema-specific conversion, validation, or nested-write failure.
    Schema(S),
    /// Output capacity or growth failure.
    Output(OutputError<G>),
}

impl<S: fmt::Display, G: fmt::Display> fmt::Display for WriteError<S, G> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Schema(error) => error.fmt(formatter),
            Self::Output(error) => error.fmt(formatter),
        }
    }
}

impl<S, G> core::error::Error for WriteError<S, G>
where
    S: core::error::Error + 'static,
    G: core::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Schema(error) => Some(error),
            Self::Output(error) => Some(error),
        }
    }
}

impl<S, G> From<OutputError<G>> for WriteError<S, G> {
    fn from(error: OutputError<G>) -> Self {
        Self::Output(error)
    }
}

/// Contiguous output used by generated progressive writers.
///
/// On successful `ensure`, `bytes()` and `bytes_mut()` must expose at least
/// `request.minimum_len` bytes and preserve `0..request.high_water_mark` byte-for-byte.
/// Implementations may relocate their backing storage. Generated code retains offsets only and
/// reacquires the mutable slice after every call.
pub trait Output {
    /// Failure produced by caller-controlled growth.
    type GrowError: core::error::Error + 'static;

    /// Current contiguous output span.
    fn bytes(&self) -> &[u8];

    /// Current mutable contiguous output span.
    fn bytes_mut(&mut self) -> &mut [u8];

    /// Exposes at least `request.minimum_len` bytes or returns an error.
    fn ensure(&mut self, request: GrowthRequest) -> Result<(), OutputError<Self::GrowError>>;
}

impl Output for &mut [u8] {
    type GrowError = Infallible;

    fn bytes(&self) -> &[u8] {
        self
    }

    fn bytes_mut(&mut self) -> &mut [u8] {
        self
    }

    fn ensure(&mut self, request: GrowthRequest) -> Result<(), OutputError<Self::GrowError>> {
        if self.len() < request.minimum_len {
            return Err(OutputError::NeedMore {
                request,
                available: self.len(),
            });
        }
        Ok(())
    }
}

impl<T> Output for &mut T
where
    T: AsRef<[u8]> + AsMut<[u8]> + Extend<u8>,
{
    type GrowError = Infallible;

    fn bytes(&self) -> &[u8] {
        self.as_ref()
    }

    fn bytes_mut(&mut self) -> &mut [u8] {
        self.as_mut()
    }

    fn ensure(&mut self, request: GrowthRequest) -> Result<(), OutputError<Self::GrowError>> {
        let current = self.as_ref().len();
        if current >= request.minimum_len {
            return Ok(());
        }
        let target = growth_target(current, request);
        self.extend(core::iter::repeat_n(0, target.saturating_sub(current)));
        verify_growth(self.as_ref().len(), request)
    }
}

/// Fixed view over an `AsRef<[u8]> + AsMut<[u8]>` owner.
pub struct Fixed<'a, T: ?Sized> {
    target: &'a mut T,
}

/// Treats the target's current visible bytes as fixed output.
pub fn fixed<T>(target: &mut T) -> Fixed<'_, T>
where
    T: ?Sized + AsRef<[u8]> + AsMut<[u8]>,
{
    Fixed { target }
}

impl<T: ?Sized> Output for Fixed<'_, T>
where
    T: AsRef<[u8]> + AsMut<[u8]>,
{
    type GrowError = Infallible;

    fn bytes(&self) -> &[u8] {
        self.target.as_ref()
    }

    fn bytes_mut(&mut self) -> &mut [u8] {
        self.target.as_mut()
    }

    fn ensure(&mut self, request: GrowthRequest) -> Result<(), OutputError<Self::GrowError>> {
        let available = self.target.as_ref().len();
        if available < request.minimum_len {
            return Err(OutputError::NeedMore { request, available });
        }
        Ok(())
    }
}

/// Growable output that refuses to expose bytes beyond a caller-provided limit.
pub struct Bounded<'a, T: ?Sized> {
    target: &'a mut T,
    limit: usize,
}

/// Allows `Extend<u8>` growth up to `limit`, useful for pooled size-class buffers.
pub fn bounded<T>(target: &mut T, limit: usize) -> Bounded<'_, T>
where
    T: ?Sized + AsRef<[u8]> + AsMut<[u8]> + Extend<u8>,
{
    Bounded { target, limit }
}

impl<T: ?Sized> Output for Bounded<'_, T>
where
    T: AsRef<[u8]> + AsMut<[u8]> + Extend<u8>,
{
    type GrowError = Infallible;

    fn bytes(&self) -> &[u8] {
        self.target.as_ref()
    }

    fn bytes_mut(&mut self) -> &mut [u8] {
        self.target.as_mut()
    }

    fn ensure(&mut self, request: GrowthRequest) -> Result<(), OutputError<Self::GrowError>> {
        if request.minimum_len > self.limit {
            return Err(OutputError::Limit {
                request,
                limit: self.limit,
            });
        }
        let current = self.target.as_ref().len();
        if current >= request.minimum_len {
            return Ok(());
        }
        let target = growth_target(current, request).min(self.limit);
        self.target
            .extend(core::iter::repeat_n(0, target.saturating_sub(current)));
        verify_growth(self.target.as_ref().len(), request)
    }
}

/// Output using a caller-provided fallible growth operation.
pub struct GrowWith<'a, T: ?Sized, F> {
    target: &'a mut T,
    grow: F,
}

/// Adds caller-controlled fallible growth without requiring an allocator dependency.
///
/// A successful callback must preserve `0..request.high_water_mark` and expose at least
/// `request.minimum_len` bytes through `AsRef` and `AsMut`.
pub fn grow_with<T, F, E>(target: &mut T, grow: F) -> GrowWith<'_, T, F>
where
    T: ?Sized + AsRef<[u8]> + AsMut<[u8]>,
    F: FnMut(&mut T, GrowthRequest) -> Result<(), E>,
    E: core::error::Error + 'static,
{
    GrowWith { target, grow }
}

impl<T: ?Sized, F, E> Output for GrowWith<'_, T, F>
where
    T: AsRef<[u8]> + AsMut<[u8]>,
    F: FnMut(&mut T, GrowthRequest) -> Result<(), E>,
    E: core::error::Error + 'static,
{
    type GrowError = E;

    fn bytes(&self) -> &[u8] {
        self.target.as_ref()
    }

    fn bytes_mut(&mut self) -> &mut [u8] {
        self.target.as_mut()
    }

    fn ensure(&mut self, request: GrowthRequest) -> Result<(), OutputError<Self::GrowError>> {
        if self.target.as_ref().len() < request.minimum_len {
            (self.grow)(self.target, request).map_err(OutputError::Grow)?;
        }
        verify_growth(self.target.as_ref().len(), request)
    }
}

/// Cursor and output owner retained by a generated typestate writer.
pub struct Writer<O> {
    output: O,
    start: usize,
    cursor: usize,
}

impl<O: Output> Writer<O> {
    /// Starts one leading representation at offset zero.
    #[must_use]
    pub fn new(output: O) -> Self {
        Self {
            output,
            start: 0,
            cursor: 0,
        }
    }

    /// Current absolute output offset.
    #[must_use]
    pub fn position(&self) -> usize {
        self.cursor
    }

    /// Writes bytes at the current cursor and advances it.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), OutputError<O::GrowError>> {
        let end = self
            .cursor
            .checked_add(bytes.len())
            .ok_or(OutputError::LengthOverflow)?;
        self.ensure(end, end)?;
        self.output.bytes_mut()[self.cursor..end].copy_from_slice(bytes);
        self.cursor = end;
        Ok(())
    }

    /// Writes bytes at an absolute offset and extends the represented range when needed.
    pub fn write_at(
        &mut self,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), OutputError<O::GrowError>> {
        let end = offset
            .checked_add(bytes.len())
            .ok_or(OutputError::LengthOverflow)?;
        self.ensure(end, end.max(self.cursor))?;
        self.output.bytes_mut()[offset..end].copy_from_slice(bytes);
        self.cursor = self.cursor.max(end);
        Ok(())
    }

    /// Copies one previously written fixed-width span.
    #[doc(hidden)]
    pub fn read_at<const N: usize>(&self, offset: usize) -> Option<[u8; N]> {
        let end = offset.checked_add(N)?;
        self.output.bytes().get(offset..end)?.try_into().ok()
    }

    /// Fills an absolute output span and extends the represented range when needed.
    #[doc(hidden)]
    pub fn fill_at(
        &mut self,
        offset: usize,
        len: usize,
        byte: u8,
    ) -> Result<(), OutputError<O::GrowError>> {
        let end = offset.checked_add(len).ok_or(OutputError::LengthOverflow)?;
        self.ensure(end, end.max(self.cursor))?;
        self.output.bytes_mut()[offset..end].fill(byte);
        self.cursor = self.cursor.max(end);
        Ok(())
    }

    /// Requests a writable span, allowing generated code to provide a larger growth hint.
    pub fn ensure(
        &mut self,
        minimum_len: usize,
        suggested_len: usize,
    ) -> Result<(), OutputError<O::GrowError>> {
        let request = GrowthRequest {
            high_water_mark: self.cursor,
            minimum_len,
            suggested_len: suggested_len.max(minimum_len),
        };
        self.output.ensure(request)
    }

    /// Creates a sequential child cursor without permitting parent-prefix overwrites.
    #[doc(hidden)]
    pub fn child_at(
        &mut self,
        position: usize,
    ) -> Result<ChildWriter<'_, O>, OutputError<O::GrowError>> {
        if position < self.cursor {
            return Err(OutputError::Backwards {
                position,
                written: self.cursor,
            });
        }
        self.ensure(position, position)?;
        self.cursor = position;
        Ok(ChildWriter {
            writer: self,
            start: position,
            cursor: position,
            limit: None,
        })
    }

    /// Creates a fixed-width child cursor that may backfill its disjoint output region.
    #[doc(hidden)]
    pub fn fixed_child_at(
        &mut self,
        position: usize,
        len: usize,
    ) -> Result<ChildWriter<'_, O>, OutputError<O::GrowError>> {
        let end = position
            .checked_add(len)
            .ok_or(OutputError::LengthOverflow)?;
        self.ensure(end, end.max(self.cursor))?;
        Ok(ChildWriter {
            writer: self,
            cursor: position,
            start: position,
            limit: Some(end),
        })
    }

    /// Finishes the current representation without clearing unused output bytes.
    #[must_use]
    pub fn finish(self) -> Written<O> {
        Written {
            output: self.output,
            range: self.start..self.cursor,
        }
    }
}

/// Restricted sequential cursor supplied to detached manual and derived children.
pub struct ChildWriter<'writer, O> {
    writer: &'writer mut Writer<O>,
    start: usize,
    cursor: usize,
    limit: Option<usize>,
}

impl<O: Output> ChildWriter<'_, O> {
    /// Current absolute output offset.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.cursor
    }

    /// Writes bytes sequentially and advances this child cursor.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), OutputError<O::GrowError>> {
        let end = self
            .cursor
            .checked_add(bytes.len())
            .ok_or(OutputError::LengthOverflow)?;
        if let Some(limit) = self.limit
            && end > limit
        {
            return Err(OutputError::ChildOverflow { end, limit });
        }
        self.writer.write_at(self.cursor, bytes)?;
        self.cursor = end;
        Ok(())
    }

    /// Zero-fills forward geometry up to an absolute position.
    #[doc(hidden)]
    pub fn fill_to(&mut self, position: usize) -> Result<(), OutputError<O::GrowError>> {
        if position < self.cursor {
            return Err(OutputError::Backwards {
                position,
                written: self.cursor,
            });
        }
        if let Some(limit) = self.limit
            && position > limit
        {
            return Err(OutputError::ChildOverflow {
                end: position,
                limit,
            });
        }
        self.writer
            .fill_at(self.cursor, position - self.cursor, 0)?;
        self.cursor = position;
        Ok(())
    }

    /// Patches bytes within the portion already emitted by this child.
    #[doc(hidden)]
    pub fn patch_at(
        &mut self,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), OutputError<O::GrowError>> {
        let end = offset
            .checked_add(bytes.len())
            .ok_or(OutputError::LengthOverflow)?;
        if offset < self.start {
            return Err(OutputError::Backwards {
                position: offset,
                written: self.start,
            });
        }
        if end > self.cursor {
            return Err(OutputError::Backwards {
                position: end,
                written: self.cursor,
            });
        }
        self.writer.write_at(offset, bytes)
    }
    /// Verifies that a fixed-width child filled its complete assigned region.
    #[doc(hidden)]
    pub fn finish(self) -> Result<(), OutputError<O::GrowError>> {
        if let Some(limit) = self.limit
            && self.cursor != limit
        {
            return Err(OutputError::ChildIncomplete {
                end: self.cursor,
                limit,
            });
        }
        Ok(())
    }
}

/// Successfully completed representation and its exact range in the output owner.
pub struct Written<O> {
    output: O,
    range: Range<usize>,
}

impl<O: Output> Written<O> {
    /// Exact represented range within the output.
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// Exact represented length.
    #[must_use]
    pub fn len(&self) -> usize {
        self.range.len()
    }

    /// Whether the representation is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.range.is_empty()
    }

    /// Immutable represented bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.output.bytes()[self.range.clone()]
    }

    /// Mutable represented bytes.
    #[must_use]
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.output.bytes_mut()[self.range.clone()]
    }

    /// Recovers the output owner and represented range.
    #[must_use]
    pub fn into_parts(self) -> (O, Range<usize>) {
        (self.output, self.range)
    }
}

fn growth_target(current: usize, request: GrowthRequest) -> usize {
    request
        .suggested_len
        .max(request.minimum_len)
        .max(current.saturating_mul(2))
}

fn verify_growth<E>(actual_len: usize, request: GrowthRequest) -> Result<(), OutputError<E>> {
    if actual_len < request.minimum_len {
        return Err(OutputError::GrowthRefused {
            request,
            actual_len,
        });
    }
    Ok(())
}
