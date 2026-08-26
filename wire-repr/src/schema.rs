//! Static schema capabilities used by the schema-only derives.

/// A successful structural frame and its retained geometry state.
#[doc(hidden)]
pub struct Frame<S> {
    state: S,
    consumed: usize,
}

impl<S> Frame<S> {
    /// Creates a validated frame.
    #[must_use]
    pub const fn new(state: S, consumed: usize) -> Self {
        Self { state, consumed }
    }

    /// Splits the retained state and represented length.
    #[must_use]
    pub fn into_parts(self) -> (S, usize) {
        (self.state, self.consumed)
    }
}

/// Static read capability for a manual or derived wire schema.
///
/// The generated public API is safe. Manual implementations are unsafe because retained geometry
/// is later paired with an immutable span without repeating framing.
///
/// # Safety
///
/// `State` must be owned and reference-free. `frame` must report an extent within `input` and
/// return geometry that remains memory-safe for any immutable span of that exact consumed length.
/// State may retain validated logical values, but it must not make unchecked semantic assumptions
/// about later input bytes.
#[allow(unsafe_code)]
pub unsafe trait WireView: Sized {
    /// Typed structural failure.
    type Error: core::error::Error + 'static;

    /// Owned, reference-free geometry retained after one framing pass.
    type State: 'static;

    /// Borrowed nested view reconstructed from retained geometry.
    type View<'view>: AsRef<[u8]>;

    /// Exact width when every representation has one compile-time width.
    const FIXED_SIZE: Option<usize>;

    /// Frames one leading representation.
    fn frame(input: &[u8], absolute_offset: usize) -> Result<Frame<Self::State>, Self::Error>;

    /// Reconstructs a child view from geometry established by framing.
    ///
    /// # Safety
    ///
    /// `state` must come from a successful `frame` whose consumed length equals `input.len()`.
    /// The address and byte contents need not match the slice originally supplied to `frame`.
    unsafe fn from_validated_parts<'view>(
        input: &'view [u8],
        state: &'view Self::State,
    ) -> Self::View<'view>;
}

/// Associates a manual or derived schema with its initial builder state.
pub trait WireBuilder: Sized {
    /// Exact width when every representation has one compile-time width.
    const FIXED_SIZE: Option<usize> = None;

    /// Initial builder type.
    type Builder;

    /// Creates an empty builder.
    fn builder() -> Self::Builder;
}

/// Writes one detached manual or derived builder value into a progressive parent writer.
pub trait WireWrite<V>: Sized {
    /// Schema-specific write failure.
    type Error: core::error::Error + 'static;

    /// Writes the represented value at the restricted sequential child cursor.
    fn write<O: crate::Output>(
        value: V,
        writer: &mut crate::ChildWriter<'_, O>,
    ) -> Result<(), crate::WriteError<Self::Error, O::GrowError>>;
}

#[doc(hidden)]
pub const fn checked_optional_sum<const N: usize>(parts: [Option<usize>; N]) -> Option<usize> {
    let mut total = 0usize;
    let mut index = 0usize;
    while index < N {
        let part = match parts[index] {
            Some(part) => part,
            None => return None,
        };
        match total.checked_add(part) {
            Some(next) => total = next,
            None => return None,
        }
        index += 1;
    }
    Some(total)
}

#[doc(hidden)]
pub const fn checked_optional_equal<const N: usize>(parts: [Option<usize>; N]) -> Option<usize> {
    let first = match parts.first() {
        Some(Some(value)) => *value,
        Some(None) | None => return None,
    };
    let mut index = 1usize;
    while index < N {
        match parts[index] {
            Some(value) if value == first => {}
            Some(_) | None => return None,
        }
        index += 1;
    }
    Some(first)
}

#[doc(hidden)]
pub const fn checked_align(position: usize, alignment: usize) -> Option<usize> {
    if alignment == 0 {
        return None;
    }
    let remainder = position % alignment;
    if remainder == 0 {
        Some(position)
    } else {
        position.checked_add(alignment - remainder)
    }
}

/// Input ended before the parser could establish one representation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("need at least {additional_at_least} more bytes at absolute offset {offset}")]
pub struct NeedMore {
    /// Absolute root-input offset where more bytes are needed.
    pub offset: usize,
    /// Proven lower bound on additional required bytes.
    pub additional_at_least: usize,
}

/// A static field offset or fixed width could not be established.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("fixed layout is unavailable or overflows before field `{field}`")]
pub struct LayoutError {
    /// Field whose physical offset could not be established.
    pub field: &'static str,
}

/// A stored scalar constant did not match its schema value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConstantMismatch<T> {
    /// Absolute root-input offset of the constant.
    pub offset: usize,
    /// Required schema value.
    pub expected: T,
    /// Stored input value.
    pub actual: T,
}

impl<T: core::fmt::Debug> core::fmt::Display for ConstantMismatch<T> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "constant mismatch at absolute offset {}: expected {:?}, got {:?}",
            self.offset, self.expected, self.actual
        )
    }
}

impl<T: core::fmt::Debug> core::error::Error for ConstantMismatch<T> {}

/// A stored scalar cannot be represented by its declared Rust type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("cannot convert scalar at absolute offset {offset} from {from} to {to}")]
pub struct ScalarConversionError {
    /// Absolute root-input offset of the scalar.
    pub offset: usize,
    /// Source wire scalar type.
    pub from: &'static str,
    /// Destination Rust type.
    pub to: &'static str,
}

/// A Rust scalar cannot be represented by its declared wire type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("cannot convert scalar from {from} to {to}")]
pub struct ScalarBuildConversionError {
    /// Source Rust type.
    pub from: &'static str,
    /// Destination wire scalar type.
    pub to: &'static str,
}

/// Exact framing found bytes after the represented value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{trailing} trailing bytes at absolute offset {offset}")]
pub struct TrailingBytes {
    /// Absolute offset of the first trailing byte.
    pub offset: usize,
    /// Number of trailing bytes.
    pub trailing: usize,
}

/// A manual child reported an extent outside the supplied input.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error(
    "invalid frame at absolute offset {offset}: consumed {consumed} bytes from {available} available"
)]
pub struct InvalidFrameExtent {
    /// Absolute start offset of the invalid child frame.
    pub offset: usize,
    /// Reported represented byte count.
    pub consumed: usize,
    /// Bytes available to that child.
    pub available: usize,
}

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
/// Exact represented bytes known to belong to schema `T`.
pub trait ExactWire<T> {
    /// Returns the complete represented byte span.
    fn as_wire_bytes(&self) -> &[u8];
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

/// Replayable facade over one counted array's exact available range.
pub struct ArrayView<'input, T: WireView> {
    input: &'input [u8],
    count: usize,
    offset: usize,
    marker: core::marker::PhantomData<fn() -> T>,
}

impl<'input, T: WireView> ArrayView<'input, T> {
    /// Creates a facade from geometry already proven by a generated parent.
    #[doc(hidden)]
    #[must_use]
    pub const fn new(input: &'input [u8], count: usize, offset: usize) -> Self {
        Self {
            input,
            count,
            offset,
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
            trailing_reported: false,
            failed: false,
            marker: core::marker::PhantomData,
        }
    }
}

/// Forward iterator produced by [`ArrayView::iter`].
pub struct ArrayIter<'input, T: WireView> {
    input: &'input [u8],
    count: usize,
    offset: usize,
    index: usize,
    cursor: usize,
    trailing_reported: bool,
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
            if self.cursor != self.input.len() && !self.trailing_reported {
                self.trailing_reported = true;
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
        let frame = match T::frame(available, absolute) {
            Ok(frame) => frame,
            Err(source) => {
                let index = self.index;
                self.failed = true;
                self.index = self.count;
                return Some(Err(ArrayError::Item { index, source }));
            }
        };
        let (state, consumed) = frame.into_parts();
        if consumed == 0 {
            let index = self.index;
            self.failed = true;
            self.index = self.count;
            return Some(Err(ArrayError::NonProgress {
                index,
                offset: absolute,
            }));
        }
        if consumed > available.len() {
            let index = self.index;
            self.failed = true;
            self.index = self.count;
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
        let frame =
            T::frame(available, absolute).map_err(|source| ArrayError::Item { index, source })?;
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

/// Empty typestate slot used by generated builders.
#[doc(hidden)]
pub struct Unset;

/// Initialized typestate slot used by generated builders.
#[doc(hidden)]
pub struct Set<T>(pub T);
#[doc(hidden)]
pub trait IsSet {}

impl<T> IsSet for Set<T> {}
