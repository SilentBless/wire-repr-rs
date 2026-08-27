//! Hidden runtime support for generated recursive views.

use crate::{ArrayError, Frame, InvalidFrameExtent, LayoutError, NeedMore, TrailingBytes};

mod geometry;
pub use geometry::{
    RecursiveGeometry, RecursiveGeometryBuilder, RecursiveMeasure, frame_recursive_array_extent,
};

/// Recursive representation exceeded the caller-selected depth.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("recursive depth {limit} exceeded at absolute offset {offset}")]
pub struct DepthExceeded {
    /// Caller-selected maximum nesting depth.
    pub limit: usize,
    /// Absolute offset of the first representation beyond the limit.
    pub offset: usize,
}

/// Runtime depth budget propagated through generated recursive framing.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecursiveDepth {
    remaining: usize,
    limit: usize,
}

impl RecursiveDepth {
    /// Creates a finite caller-selected budget.
    #[must_use]
    pub const fn new(limit: usize) -> Self {
        Self {
            remaining: limit,
            limit,
        }
    }

    /// Returns the configured limit.
    #[must_use]
    pub const fn limit(self) -> usize {
        self.limit
    }

    /// Returns levels still available below the current parent.
    #[must_use]
    pub const fn remaining(self) -> usize {
        self.remaining
    }

    /// Enters one root and returns the budget available to its body.
    pub fn enter(self, offset: usize) -> Result<Self, DepthExceeded> {
        if self.remaining == 0 {
            return Err(DepthExceeded {
                limit: self.limit,
                offset,
            });
        }
        Ok(Self {
            remaining: self.remaining - 1,
            limit: self.limit,
        })
    }
}

/// Finite error shared by recursively repeated child boundaries.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RecursiveError {
    /// Input ended before a representation could be framed.
    #[error(transparent)]
    NeedMore(#[from] NeedMore),
    /// Required physical geometry could not be represented.
    #[error(transparent)]
    Layout(#[from] LayoutError),
    /// A generated child reported an invalid extent.
    #[error(transparent)]
    InvalidFrame(#[from] InvalidFrameExtent),
    /// Exact framing left bytes inside a declared body.
    #[error(transparent)]
    Trailing(#[from] TrailingBytes),
    /// The caller-selected recursive depth was exceeded.
    #[error(transparent)]
    DepthExceeded(#[from] DepthExceeded),
    /// A schema-specific child failed at this absolute site.
    #[error("recursive child failed at absolute offset {offset}")]
    Child {
        /// Absolute child start.
        offset: usize,
    },
    /// A recursive selector was not recognized.
    #[error("unknown recursive selector at absolute offset {offset}")]
    UnknownSelector {
        /// Absolute selector start.
        offset: usize,
    },
}

/// Flattens generated root errors at recursive repetition boundaries.
#[doc(hidden)]
pub trait FlattenRecursiveError {
    /// Converts the generated error without retaining recursive type nesting.
    fn flatten_recursive(self, fallback_offset: usize) -> RecursiveError;
}

impl<E> FlattenRecursiveError for ArrayError<E>
where
    E: core::error::Error + FlattenRecursiveError + 'static,
{
    fn flatten_recursive(self, fallback_offset: usize) -> RecursiveError {
        match self {
            ArrayError::NeedMore(source) => RecursiveError::NeedMore(source),
            ArrayError::Item { source, .. } => source.flatten_recursive(fallback_offset),
            ArrayError::InvalidExtent {
                consumed,
                available,
                ..
            } => RecursiveError::InvalidFrame(InvalidFrameExtent {
                offset: fallback_offset,
                consumed,
                available,
            }),
            ArrayError::NonProgress { offset, .. } => RecursiveError::Child { offset },
            ArrayError::Trailing { offset, trailing } => {
                RecursiveError::Trailing(TrailingBytes { offset, trailing })
            }
        }
    }
}
/// One transition in a generated iterative recursive-body grammar.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecursiveStep {
    /// The body completes after advancing over one final nonrecursive segment.
    Done {
        /// Bytes consumed before body completion.
        advance: usize,
    },
    /// The body advances to one recursive child and retains a continuation token.
    Child {
        /// Bytes consumed before the child root begins.
        advance: usize,
        /// Body-local token resumed after the child completes.
        continuation: u32,
    },
}

/// Resolves a generated recursive slot without reconstructing hidden item names.
#[doc(hidden)]
pub trait RecursiveSlot<const INDEX: usize> {
    /// Hidden marker selected for this concrete body instantiation.
    type Marker;
}

/// Statically dispatched recursive-root callback generated by the root enum.
#[doc(hidden)]
#[allow(unsafe_code)]
pub trait RecursiveFrame<Slot> {
    /// Recursive schema root.
    type Root;
    /// Finite state for one root.
    type State: 'static;
    /// Finite generated root error.
    type Error: core::error::Error + FlattenRecursiveError + 'static;
    /// Ordinary generated root view family.
    type View<'view, const DEPTH: usize>: AsRef<[u8]>;

    /// Frames one root after its depth level has already been entered.
    fn frame<const DEPTH: usize>(
        input: &[u8],
        absolute_offset: usize,
        body_depth: RecursiveDepth,
    ) -> Result<Frame<Self::State>, Self::Error>;

    /// Skips exactly one root through an iterative generated grammar machine.
    fn skip<const DEPTH: usize>(
        input: &[u8],
        absolute_offset: usize,
        depth: RecursiveDepth,
    ) -> Result<RecursiveMeasure, Self::Error>;

    /// Reconstructs the same generated root view family over one exact item span.
    ///
    /// # Safety
    /// `state` must come from `frame` over `input` with the supplied offset and body depth.
    unsafe fn into_view<'view, const DEPTH: usize>(
        input: &'view [u8],
        state: Self::State,
        absolute_offset: usize,
        body_depth: RecursiveDepth,
    ) -> Self::View<'view, DEPTH>;
}

/// Hidden recursive framing surface generated for a generic body slot.
#[doc(hidden)]
#[allow(unsafe_code)]
pub trait RecursiveBody<C, Slot>: Sized {
    /// Finite local body state. Recursive child states are never retained here.
    type State: 'static;
    /// Finite body error.
    type Error: core::error::Error + 'static;
    /// Body view borrowing the immutable root item span.
    type View<'view, const DEPTH: usize>: AsRef<[u8]>;

    /// Starts the generated body machine before its first recursive child.
    fn recursive_start(input: &[u8], absolute_offset: usize) -> Result<RecursiveStep, Self::Error>;

    /// Resumes the generated body machine after one recursive child completed.
    fn recursive_resume(
        input: &[u8],
        absolute_offset: usize,
        continuation: u32,
    ) -> Result<RecursiveStep, Self::Error>;

    /// Frames the complete body and builds compact exact item geometry.
    fn frame_recursive<const DEPTH: usize>(
        input: &[u8],
        absolute_offset: usize,
        depth: RecursiveDepth,
    ) -> Result<Frame<Self::State>, Self::Error>
    where
        C: RecursiveFrame<Slot>;

    /// Reconstructs the body view from its exact span and finite state.
    ///
    /// # Safety
    /// `state` must come from `frame_recursive` for this exact body span.
    unsafe fn from_recursive_parts<'view, const DEPTH: usize>(
        input: &'view [u8],
        state: &'view Self::State,
        absolute_offset: usize,
        depth: RecursiveDepth,
    ) -> Self::View<'view, DEPTH>
    where
        C: RecursiveFrame<Slot>;
}
/// Finite schema failure crossing a progressive recursive-write boundary.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RecursiveWriteError {
    /// An ordinary leaf or nested recursive body failed.
    #[error("recursive child `{field}` failed")]
    Child {
        /// Variant or field that owned the failing child.
        field: &'static str,
    },
    /// A streamed recursive collection exceeded the recursive count limit or stored representation.
    #[error(
        "recursive collection `{field}` count {count} exceeds its recursive limit or controller"
    )]
    CountOverflow {
        /// Counted collection field.
        field: &'static str,
        /// Emitted item count.
        count: usize,
    },
}

/// Associates a generic recursive body with its generated progressive-write slot marker.
#[doc(hidden)]
pub trait RecursiveWriteSlot<const INDEX: usize> {
    /// Marker statically binding a body grammar to one recursive root callback.
    type Marker;
}

/// Restricted progressive cursor accepted by generated recursive writers.
#[doc(hidden)]
pub trait RecursiveCursor {
    /// Caller-controlled output growth failure.
    type GrowError: core::error::Error + 'static;

    /// Current absolute output offset.
    fn position(&self) -> usize;

    /// Writes bytes sequentially.
    fn write(&mut self, bytes: &[u8]) -> Result<(), crate::OutputError<Self::GrowError>>;

    /// Patches bytes already emitted by this cursor.
    fn patch_at(
        &mut self,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), crate::OutputError<Self::GrowError>>;

    /// Writes one detached ordinary child through its existing schema capability.
    fn write_value<Schema, Value>(
        &mut self,
        value: Value,
    ) -> Result<(), crate::WriteError<Schema::Error, Self::GrowError>>
    where
        Schema: crate::WireWrite<Value>;
}

impl<O: crate::Output> RecursiveCursor for crate::ChildWriter<'_, O> {
    type GrowError = O::GrowError;

    fn position(&self) -> usize {
        crate::ChildWriter::position(self)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), crate::OutputError<Self::GrowError>> {
        crate::ChildWriter::write(self, bytes)
    }

    fn patch_at(
        &mut self,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), crate::OutputError<Self::GrowError>> {
        crate::ChildWriter::patch_at(self, offset, bytes)
    }

    fn write_value<Schema, Value>(
        &mut self,
        value: Value,
    ) -> Result<(), crate::WriteError<Schema::Error, Self::GrowError>>
    where
        Schema: crate::WireWrite<Value>,
    {
        Schema::write(value, self)
    }
}

impl<O: crate::Output> RecursiveCursor for crate::Writer<O> {
    type GrowError = O::GrowError;

    fn position(&self) -> usize {
        crate::Writer::position(self)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), crate::OutputError<Self::GrowError>> {
        crate::Writer::write(self, bytes)
    }

    fn patch_at(
        &mut self,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), crate::OutputError<Self::GrowError>> {
        let end = offset
            .checked_add(bytes.len())
            .ok_or(crate::OutputError::LengthOverflow)?;
        if end > self.position() {
            return Err(crate::OutputError::Backwards {
                position: end,
                written: self.position(),
            });
        }
        crate::Writer::write_at(self, offset, bytes)
    }

    fn write_value<Schema, Value>(
        &mut self,
        value: Value,
    ) -> Result<(), crate::WriteError<Schema::Error, Self::GrowError>>
    where
        Schema: crate::WireWrite<Value>,
    {
        let start = self.position();
        let mut child = self.child_at(start)?;
        Schema::write(value, &mut child)?;
        child.finish()?;
        Ok(())
    }
}

/// Generated progressive writer callback for one recursive root.
#[doc(hidden)]
pub trait RecursiveWrite {
    /// Initial recursive root writer owning one progressive cursor.
    type Writer<Cursor: RecursiveCursor>;

    /// Completed recursive root writer returning the same cursor.
    type Complete<Cursor: RecursiveCursor>;

    /// Transfers one cursor into this recursive root writer.
    fn writer<Cursor: RecursiveCursor>(
        output: Cursor,
    ) -> Result<Self::Writer<Cursor>, crate::WriteError<RecursiveWriteError, Cursor::GrowError>>;

    /// Returns the cursor after one recursive root completed.
    fn finish<Cursor: RecursiveCursor>(
        complete: Self::Complete<Cursor>,
    ) -> Result<Cursor, crate::WriteError<RecursiveWriteError, Cursor::GrowError>>;
}

/// Generated progressive writer for one recursive body grammar.
#[doc(hidden)]
pub trait RecursiveWriteBody<Callback, Slot> {
    /// Initial body writer owning one progressive cursor.
    type Writer<Cursor: RecursiveCursor>;

    /// Completed body writer returning the same cursor.
    type Complete<Cursor: RecursiveCursor>;

    /// Transfers one cursor into this body writer.
    fn writer<Cursor: RecursiveCursor>(
        output: Cursor,
    ) -> Result<Self::Writer<Cursor>, crate::WriteError<RecursiveWriteError, Cursor::GrowError>>;

    /// Returns the cursor after every required body field was written.
    fn finish<Cursor: RecursiveCursor>(
        complete: Self::Complete<Cursor>,
    ) -> Result<Cursor, crate::WriteError<RecursiveWriteError, Cursor::GrowError>>;
}

/// Flattens a recursive array facade error without retaining recursive generic error types.
#[doc(hidden)]
pub fn flatten_recursive_array_error(
    error: ArrayError<RecursiveError>,
    fallback_offset: usize,
) -> RecursiveError {
    match error {
        ArrayError::NeedMore(source) => RecursiveError::NeedMore(source),
        ArrayError::Item { source, .. } => source,
        ArrayError::InvalidExtent {
            consumed,
            available,
            ..
        } => RecursiveError::InvalidFrame(InvalidFrameExtent {
            offset: fallback_offset,
            consumed,
            available,
        }),
        ArrayError::NonProgress { offset, .. } => RecursiveError::Child { offset },
        ArrayError::Trailing { offset, trailing } => {
            RecursiveError::Trailing(TrailingBytes { offset, trailing })
        }
    }
}

/// Replayable recursive array facade yielding the root's ordinary view family.
#[doc(hidden)]
pub struct RecursiveArrayView<'input, 'geometry, C, Slot, const DEPTH: usize>
where
    C: RecursiveFrame<Slot>,
{
    input: &'input [u8],
    count: usize,
    offset: usize,
    depth: RecursiveDepth,
    geometry: &'geometry RecursiveGeometry,
    marker: core::marker::PhantomData<fn() -> (C, Slot)>,
}

impl<'input, 'geometry, C, Slot, const DEPTH: usize>
    RecursiveArrayView<'input, 'geometry, C, Slot, DEPTH>
where
    C: RecursiveFrame<Slot>,
{
    /// Creates a facade over geometry proven by generated body framing.
    ///
    /// # Safety
    /// The span must be the exact concatenation of `count` items framed through `C`.
    #[doc(hidden)]
    #[must_use]
    #[allow(unsafe_code)]
    pub const unsafe fn from_validated_parts(
        input: &'input [u8],
        count: usize,
        offset: usize,
        depth: RecursiveDepth,
        geometry: &'geometry RecursiveGeometry,
    ) -> Self {
        Self {
            input,
            count,
            offset,
            depth,
            geometry,
            marker: core::marker::PhantomData,
        }
    }

    /// Returns the authoritative item count.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Reports whether the array is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Reports the exact lookup strategy retained by parent framing.
    #[doc(hidden)]
    #[must_use]
    pub const fn geometry_kind(&self) -> &'static str {
        self.geometry.kind()
    }

    /// Returns one exact recursive root view by ordinal.
    #[inline(always)]
    #[allow(unsafe_code)]
    pub fn get(
        &self,
        requested: usize,
    ) -> Result<Option<C::View<'input, DEPTH>>, ArrayError<RecursiveError>> {
        let Some(range) = self.geometry.span::<C, Slot, DEPTH>(
            self.input,
            self.count,
            self.offset,
            self.depth,
            requested,
        )?
        else {
            return Ok(None);
        };
        let absolute = self
            .offset
            .checked_add(range.start)
            .ok_or(ArrayError::InvalidExtent {
                index: requested,
                consumed: usize::MAX,
                available: self.input.len().saturating_sub(range.start),
            })?;
        let body_depth = self
            .depth
            .enter(absolute)
            .map_err(|source| ArrayError::Item {
                index: requested,
                source: RecursiveError::DepthExceeded(source),
            })?;
        let item = &self.input[range.clone()];
        let frame =
            C::frame::<DEPTH>(item, absolute, body_depth).map_err(|source| ArrayError::Item {
                index: requested,
                source: source.flatten_recursive(absolute),
            })?;
        let (state, consumed) = frame.into_parts();
        if consumed != item.len() {
            return Err(ArrayError::InvalidExtent {
                index: requested,
                consumed,
                available: item.len(),
            });
        }
        // SAFETY: `state` came from framing this exact item span and body depth.
        Ok(Some(unsafe {
            C::into_view::<DEPTH>(item, state, absolute, body_depth)
        }))
    }

    /// Starts a fresh forward traversal with one physical cursor.
    #[must_use]
    pub const fn iter(&self) -> RecursiveArrayIter<'input, C, Slot, DEPTH> {
        RecursiveArrayIter {
            input: self.input,
            count: self.count,
            offset: self.offset,
            depth: self.depth,
            index: 0,
            cursor: 0,
            failed: false,
            marker: core::marker::PhantomData,
        }
    }
}

/// Forward iterator over recursively framed root values.
#[doc(hidden)]
pub struct RecursiveArrayIter<'input, C, Slot, const DEPTH: usize>
where
    C: RecursiveFrame<Slot>,
{
    input: &'input [u8],
    count: usize,
    offset: usize,
    depth: RecursiveDepth,
    index: usize,
    cursor: usize,
    failed: bool,
    marker: core::marker::PhantomData<fn() -> (C, Slot)>,
}

impl<'input, C, Slot, const DEPTH: usize> Iterator for RecursiveArrayIter<'input, C, Slot, DEPTH>
where
    C: RecursiveFrame<Slot>,
{
    type Item = Result<C::View<'input, DEPTH>, ArrayError<RecursiveError>>;
    #[inline(always)]
    #[allow(unsafe_code)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        if self.index == self.count {
            if self.cursor == self.input.len() {
                return None;
            }
            self.failed = true;
            return Some(Err(ArrayError::Trailing {
                offset: self.offset.saturating_add(self.cursor),
                trailing: self.input.len() - self.cursor,
            }));
        }
        let index = self.index;
        let absolute = match self.offset.checked_add(self.cursor) {
            Some(absolute) => absolute,
            None => {
                self.failed = true;
                return Some(Err(ArrayError::InvalidExtent {
                    index,
                    consumed: usize::MAX,
                    available: self.input.len().saturating_sub(self.cursor),
                }));
            }
        };
        let body_depth = match self.depth.enter(absolute) {
            Ok(depth) => depth,
            Err(source) => {
                self.failed = true;
                return Some(Err(ArrayError::Item {
                    index,
                    source: RecursiveError::DepthExceeded(source),
                }));
            }
        };
        let available = &self.input[self.cursor..];
        let frame = match C::frame::<DEPTH>(available, absolute, body_depth) {
            Ok(frame) => frame,
            Err(source) => {
                self.failed = true;
                return Some(Err(ArrayError::Item {
                    index,
                    source: source.flatten_recursive(absolute),
                }));
            }
        };
        let (state, consumed) = frame.into_parts();
        if consumed == 0 || consumed > available.len() {
            self.failed = true;
            return Some(Err(ArrayError::InvalidExtent {
                index,
                consumed,
                available: available.len(),
            }));
        }
        let start = self.cursor;
        self.cursor += consumed;
        self.index += 1;
        let item = &self.input[start..self.cursor];
        // SAFETY: state and depth came from framing this exact item.
        Some(Ok(unsafe {
            C::into_view::<DEPTH>(item, state, absolute, body_depth)
        }))
    }
}
