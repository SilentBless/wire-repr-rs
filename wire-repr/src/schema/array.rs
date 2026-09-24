use super::{ExactWire, NeedMore, WireView};

/// Lazy failure while traversing a counted runtime array.
#[derive(Debug, thiserror::Error)]
pub enum ArrayError<E> {
    /// The available input ended before the array's proven extent.
    #[error("{0}")]
    NeedMore(#[source] NeedMore),
    /// One item failed its own structural framing.
    #[error("array item {index} failed: {source}")]
    Item {
        /// Zero-based item index.
        index: usize,
        /// Concrete item framing error.
        #[source]
        source: E,
    },
    /// An item reported an extent outside its available suffix.
    #[error("array item {index} consumed {consumed} bytes from {available} available")]
    InvalidExtent {
        /// Zero-based item index.
        index: usize,
        /// Reported item length.
        consumed: usize,
        /// Available suffix length.
        available: usize,
    },
    /// A variable item did not advance the collection cursor.
    #[error("array item {index} consumed zero bytes at absolute offset {offset}")]
    NonProgress {
        /// Zero-based item index.
        index: usize,
        /// Absolute item start offset.
        offset: usize,
    },
    /// The authoritative count left bytes inside the declared array range.
    #[error("{trailing} trailing array bytes at absolute offset {offset}")]
    Trailing {
        /// Absolute offset of the first trailing byte.
        offset: usize,
        /// Remaining bytes.
        trailing: usize,
    },
}
/// One exact counted-array item retaining its own framing state.
pub struct ArrayItem<'input, T: WireView> {
    input: &'input [u8],
    state: T::State,
}

impl<'input, T: WireView> ArrayItem<'input, T> {
    /// Returns this item's exact represented bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.input
    }

    /// Reconstructs the item's ordinary borrowed generated view.
    #[must_use]
    #[allow(unsafe_code)]
    pub fn view(&self) -> T::View<'_> {
        // SAFETY: `state` was produced by framing this exact `input` span below.
        unsafe { T::from_validated_parts(self.input, &self.state) }
    }
}

impl<'input, T: WireView> ExactWire<T> for ArrayItem<'input, T> {
    fn as_wire_bytes(&self) -> &[u8] {
        self.input
    }
}

/// Replayable facade over one counted array's available range.
pub struct ArrayView<'input, T: WireView> {
    input: &'input [u8],
    count: usize,
    offset: usize,
    validated_extent: bool,
    marker: core::marker::PhantomData<fn() -> T>,
}

impl<'input, T: WireView> ArrayView<'input, T> {
    /// Creates a terminal facade whose item geometry remains deferred.
    #[doc(hidden)]
    #[must_use]
    pub const fn terminal(input: &'input [u8], count: usize, offset: usize) -> Self {
        Self {
            input,
            count,
            offset,
            validated_extent: false,
            marker: core::marker::PhantomData,
        }
    }
    /// Creates a facade whose complete outer geometry was proven by its generated parent.
    ///
    /// # Safety
    /// For variable-width `T`, every item must have framed successfully and consumed this complete
    /// span. For fixed-width `T`, `input.len()` must equal `count * T::FIXED_SIZE`; item validation
    /// may remain deferred.
    #[doc(hidden)]
    #[must_use]
    #[allow(unsafe_code)]
    pub const unsafe fn from_validated_parts(
        input: &'input [u8],
        count: usize,
        offset: usize,
    ) -> Self {
        Self {
            input,
            count,
            offset,
            validated_extent: true,
            marker: core::marker::PhantomData,
        }
    }

    /// Returns the authoritative stored item count.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Reports whether the authoritative count is zero.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Starts a fresh forward traversal from the first item.
    #[must_use]
    pub const fn iter(&self) -> ArrayIter<'input, T> {
        ArrayIter {
            input: self.input,
            count: self.count,
            offset: self.offset,
            index: 0,
            cursor: 0,
            failed: false,
            marker: core::marker::PhantomData,
        }
    }
    #[inline]
    pub(crate) fn exact_bytes(&self) -> Result<&'input [u8], ArrayError<T::Error>> {
        if !self.validated_extent {
            let consumed = frame_array_extent::<T>(self.input, self.count, self.offset)?;
            if consumed != self.input.len() {
                return Err(ArrayError::Trailing {
                    offset: self.offset.saturating_add(consumed),
                    trailing: self.input.len() - consumed,
                });
            }
            if T::FIXED_SIZE.is_none() {
                return Ok(self.input);
            }
        } else if T::FIXED_SIZE.is_none() {
            return Ok(self.input);
        }

        let consumed = frame_array_items::<T>(self.input, self.count, self.offset)?;
        debug_assert_eq!(consumed, self.input.len());
        Ok(self.input)
    }
}

impl<'input, T: WireView> IntoIterator for ArrayView<'input, T> {
    type Item = Result<ArrayItem<'input, T>, ArrayError<T::Error>>;
    type IntoIter = ArrayIter<'input, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'input, T: WireView> IntoIterator for &ArrayView<'input, T> {
    type Item = Result<ArrayItem<'input, T>, ArrayError<T::Error>>;
    type IntoIter = ArrayIter<'input, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Forward iterator produced by [`ArrayView::iter`].
pub struct ArrayIter<'input, T: WireView> {
    input: &'input [u8],
    count: usize,
    offset: usize,
    index: usize,
    cursor: usize,
    failed: bool,
    marker: core::marker::PhantomData<fn() -> T>,
}

impl<'input, T: WireView> Iterator for ArrayIter<'input, T> {
    type Item = Result<ArrayItem<'input, T>, ArrayError<T::Error>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        if self.index == self.count {
            if self.cursor != self.input.len() {
                self.failed = true;
                return Some(Err(ArrayError::Trailing {
                    offset: self.offset.saturating_add(self.cursor),
                    trailing: self.input.len() - self.cursor,
                }));
            }
            return None;
        }
        let available = &self.input[self.cursor..];
        let absolute = match self.offset.checked_add(self.cursor) {
            Some(absolute) => absolute,
            None => {
                self.failed = true;
                return Some(Err(ArrayError::InvalidExtent {
                    index: self.index,
                    consumed: usize::MAX,
                    available: available.len(),
                }));
            }
        };
        let frame_input = match T::FIXED_SIZE {
            Some(width) if width != 0 => available.get(..width).unwrap_or(available),
            _ => available,
        };
        let frame = match if T::LEADING_EXTENT {
            T::frame(frame_input, absolute)
        } else {
            T::frame_exact(frame_input, absolute)
        } {
            Ok(frame) => frame,
            Err(source) => {
                let index = self.index;
                self.failed = true;
                return Some(Err(ArrayError::Item { index, source }));
            }
        };
        let (state, consumed) = frame.into_parts();
        if consumed == 0 {
            let index = self.index;
            self.failed = true;
            return Some(Err(ArrayError::NonProgress {
                index,
                offset: absolute,
            }));
        }
        let expected = T::FIXED_SIZE.unwrap_or(consumed);
        if consumed != expected || consumed > available.len() {
            let index = self.index;
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
        Some(Ok(ArrayItem {
            input: &self.input[start..self.cursor],
            state,
        }))
    }
}
impl<T: WireView> core::iter::FusedIterator for ArrayIter<'_, T> {}

/// Computes the exact extent needed to reach a field after a counted array.
#[doc(hidden)]
pub fn frame_array_extent<T: WireView>(
    input: &[u8],
    count: usize,
    offset: usize,
) -> Result<usize, ArrayError<T::Error>> {
    if let Some(width) = T::FIXED_SIZE {
        if width == 0 && count != 0 {
            return Err(ArrayError::NonProgress { index: 0, offset });
        }
        let consumed = width.checked_mul(count).ok_or(ArrayError::InvalidExtent {
            index: count,
            consumed: usize::MAX,
            available: input.len(),
        })?;
        if consumed > input.len() {
            return Err(ArrayError::NeedMore(NeedMore {
                offset: offset.saturating_add(input.len()),
                additional_at_least: consumed - input.len(),
            }));
        }
        return Ok(consumed);
    }
    frame_array_items::<T>(input, count, offset)
}

fn frame_array_items<T: WireView>(
    input: &[u8],
    count: usize,
    offset: usize,
) -> Result<usize, ArrayError<T::Error>> {
    let mut cursor = 0usize;
    for index in 0..count {
        let available = &input[cursor..];
        let absolute = offset
            .checked_add(cursor)
            .ok_or(ArrayError::InvalidExtent {
                index,
                consumed: usize::MAX,
                available: available.len(),
            })?;
        let frame = if T::LEADING_EXTENT {
            T::frame(available, absolute)
        } else {
            T::frame_exact(available, absolute)
        }
        .map_err(|source| ArrayError::Item { index, source })?;
        let (_, consumed) = frame.into_parts();
        if consumed == 0 {
            return Err(ArrayError::NonProgress {
                index,
                offset: absolute,
            });
        }
        if consumed > available.len() {
            return Err(ArrayError::InvalidExtent {
                index,
                consumed,
                available: available.len(),
            });
        }
        cursor += consumed;
    }
    Ok(cursor)
}
