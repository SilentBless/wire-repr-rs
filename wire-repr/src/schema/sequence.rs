use super::{InvalidFrameExtent, WireFields, WireView};

#[doc(hidden)]
pub trait WireSelect: WireView {
    type Root<B>: WireFields
    where
        B: AsRef<[u8]>;

    fn select_view<B: AsRef<[u8]>>(input: B) -> Result<Self::Root<B>, Self::Error>;
    fn validated_view<B: AsRef<[u8]>>(input: B) -> Result<Self::Root<B>, Self::Error>;
    /// # Safety
    /// `state` must come from a successful frame whose consumed length equals
    /// `input.as_ref().len()`.
    #[allow(unsafe_code)]
    unsafe fn framed_view<B: AsRef<[u8]>>(input: B, state: Self::State) -> Self::Root<B>;
    fn validate_view<B: AsRef<[u8]>>(view: &Self::Root<B>) -> Result<(), Self::Error>;
}

/// Marker for schemas whose leading framing is deterministic.
///
/// # Safety
/// A nonzero fixed width must reframe the same immutable item at the same absolute offset without
/// failing or changing its consumed extent after prevalidation. Variable framing must report one
/// valid bounded prefix and must not consume an unrelated suffix.
#[allow(unsafe_code)]
pub unsafe trait LeadingWire: WireSelect {}

/// Failure while framing one item from a sequence or cursor.
#[derive(Debug, thiserror::Error)]
pub enum SequenceError<E: core::error::Error + 'static> {
    /// The schema rejected the next item.
    #[error("wire item failed: {0}")]
    Schema(#[source] E),
    /// The schema cannot determine a leading item boundary.
    #[error("schema has no decodable leading extent")]
    Unavailable,
    /// A manual frame reported an extent beyond the available suffix.
    #[error(transparent)]
    InvalidFrame(#[from] InvalidFrameExtent),
    /// A supposedly consumable schema made no forward progress.
    #[error("wire item at byte offset {offset} consumed no bytes")]
    NonProgress {
        /// Absolute input offset.
        offset: usize,
    },
}

/// Prevalidated exact-size iterator for one fixed-width schema.
pub struct FixedViews<'input, S: LeadingWire> {
    input: &'input [u8],
    width: usize,
    index: usize,
    count: usize,
    marker: core::marker::PhantomData<fn() -> S>,
}

impl<'input, S: LeadingWire> FixedViews<'input, S> {
    #[doc(hidden)]
    #[allow(unsafe_code)]
    pub fn new(input: &'input [u8]) -> Result<Self, SequenceError<S::Error>> {
        let width = S::FIXED_SIZE.ok_or(SequenceError::NonProgress { offset: 0 })?;
        if width == 0 {
            return Err(SequenceError::NonProgress { offset: 0 });
        }
        let mut offset = 0usize;
        while offset < input.len() {
            let frame = S::frame(&input[offset..], offset).map_err(SequenceError::Schema)?;
            let (state, consumed) = frame.into_parts();
            if consumed > input.len() - offset {
                return Err(SequenceError::InvalidFrame(InvalidFrameExtent {
                    offset,
                    consumed,
                    available: input.len() - offset,
                }));
            }
            if consumed != width {
                return Err(SequenceError::InvalidFrame(InvalidFrameExtent {
                    offset,
                    consumed,
                    available: width,
                }));
            }
            // SAFETY: `state` came from framing this exact width-bounded span above.
            let view = unsafe { S::framed_view(&input[offset..offset + width], state) };
            S::validate_view(&view).map_err(SequenceError::Schema)?;
            offset += width;
        }
        Ok(Self {
            input,
            width,
            index: 0,
            count: input.len() / width,
            marker: core::marker::PhantomData,
        })
    }
}

impl<'input, S: LeadingWire> Iterator for FixedViews<'input, S> {
    type Item = S::Root<&'input [u8]>;

    #[allow(unsafe_code)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.index == self.count {
            return None;
        }
        let start = self.index * self.width;
        self.index += 1;
        let input = &self.input[start..start + self.width];
        let frame = S::frame(input, start)
            .unwrap_or_else(|_| unreachable!("fixed views were prevalidated"));
        let (state, consumed) = frame.into_parts();
        assert_eq!(
            consumed, self.width,
            "fixed frame changed after prevalidation"
        );
        // SAFETY: `state` came from re-framing this exact fixed-width span.
        Some(unsafe { S::framed_view(input, state) })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.count - self.index;
        (remaining, Some(remaining))
    }
}

impl<S: LeadingWire> ExactSizeIterator for FixedViews<'_, S> {
    fn len(&self) -> usize {
        self.count - self.index
    }
}

impl<S: LeadingWire> core::iter::FusedIterator for FixedViews<'_, S> {}

/// Result of advancing one lazy variable-width sequence facade.
pub type NextView<'input, S> =
    Result<Option<<S as WireSelect>::Root<&'input [u8]>>, SequenceError<<S as WireView>::Error>>;

/// Lazy facade over consecutive variable-width representations.
pub struct VariableViews<'input, S: WireSelect> {
    input: &'input [u8],
    offset: usize,
    marker: core::marker::PhantomData<fn() -> S>,
}

impl<'input, S: WireSelect> VariableViews<'input, S> {
    #[doc(hidden)]
    #[must_use]
    pub const fn new(input: &'input [u8]) -> Self {
        Self {
            input,
            offset: 0,
            marker: core::marker::PhantomData,
        }
    }

    /// Current absolute position.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.offset
    }

    /// Remaining unframed input.
    #[must_use]
    pub fn remaining(&self) -> &'input [u8] {
        &self.input[self.offset..]
    }

    /// Frames the next exact item lazily.
    #[allow(clippy::should_implement_trait)]
    #[allow(unsafe_code)]
    pub fn next(&mut self) -> NextView<'input, S> {
        if !S::LEADING_EXTENT {
            return Err(SequenceError::Unavailable);
        }
        if self.offset == self.input.len() {
            return Ok(None);
        }
        let start = self.offset;
        let frame = S::frame(&self.input[start..], start).map_err(SequenceError::Schema)?;
        let (state, consumed) = frame.into_parts();
        if consumed == 0 {
            return Err(SequenceError::NonProgress { offset: start });
        }
        if consumed > self.input.len() - start {
            return Err(SequenceError::InvalidFrame(InvalidFrameExtent {
                offset: start,
                consumed,
                available: self.input.len() - start,
            }));
        }
        let end = start + consumed;
        // SAFETY: extent checks prove this is the exact span that produced `state`.
        let view = unsafe { S::framed_view(&self.input[start..end], state) };
        S::validate_view(&view).map_err(SequenceError::Schema)?;
        self.offset = end;
        Ok(Some(view))
    }
}

/// Position retained while heterogeneous schemas consume one backing slice.
pub struct Cursor<'input> {
    input: &'input [u8],
    offset: usize,
}

impl<'input> Cursor<'input> {
    /// Starts at the first byte of `input`.
    #[must_use]
    pub const fn new(input: &'input [u8]) -> Self {
        Self { input, offset: 0 }
    }

    /// Remaining unconsumed input.
    #[must_use]
    pub fn remaining(&self) -> &'input [u8] {
        &self.input[self.offset..]
    }

    /// Current absolute input position.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.offset
    }

    #[doc(hidden)]
    #[allow(unsafe_code)]
    pub fn read<S: WireSelect>(
        &mut self,
    ) -> Result<S::Root<&'input [u8]>, SequenceError<S::Error>> {
        if !S::LEADING_EXTENT {
            return Err(SequenceError::Unavailable);
        }
        let start = self.offset;
        let frame = S::frame(&self.input[start..], start).map_err(SequenceError::Schema)?;
        let (state, consumed) = frame.into_parts();
        if consumed == 0 {
            return Err(SequenceError::NonProgress { offset: start });
        }
        if consumed > self.input.len() - start {
            return Err(SequenceError::InvalidFrame(InvalidFrameExtent {
                offset: start,
                consumed,
                available: self.input.len() - start,
            }));
        }
        let end = start + consumed;
        // SAFETY: extent checks prove this is the exact span that produced `state`.
        let view = unsafe { S::framed_view(&self.input[start..end], state) };
        S::validate_view(&view).map_err(SequenceError::Schema)?;
        self.offset = end;
        Ok(view)
    }
}
