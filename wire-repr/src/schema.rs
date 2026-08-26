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

    /// Whether framing can determine one leading representation without consuming the suffix.
    const LEADING_EXTENT: bool = Self::FIXED_SIZE.is_some();

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
        let frame = match T::frame(frame_input, absolute) {
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

/// One generated physical field in a typed selection expression.
#[derive(Clone, Copy)]
pub struct FieldPath {
    index: usize,
}

impl FieldPath {
    #[doc(hidden)]
    #[must_use]
    pub const fn new(index: usize) -> Self {
        Self { index }
    }
}

/// Type-level union of two physical selection expressions.
pub struct FieldUnion<L, R> {
    left: L,
    right: R,
}

#[doc(hidden)]
pub trait FieldExpr {
    fn contains(&self, index: usize) -> bool;
}

impl FieldExpr for FieldPath {
    #[inline(always)]
    fn contains(&self, index: usize) -> bool {
        self.index == index
    }
}

impl<L: FieldExpr, R: FieldExpr> FieldExpr for FieldUnion<L, R> {
    #[inline(always)]
    fn contains(&self, index: usize) -> bool {
        self.left.contains(index) || self.right.contains(index)
    }
}

impl<R: FieldExpr> core::ops::BitOr<R> for FieldPath {
    type Output = FieldUnion<Self, R>;

    fn bitor(self, right: R) -> Self::Output {
        FieldUnion { left: self, right }
    }
}

impl<L: FieldExpr, R: FieldExpr, Next: FieldExpr> core::ops::BitOr<Next> for FieldUnion<L, R> {
    type Output = FieldUnion<Self, Next>;

    fn bitor(self, right: Next) -> Self::Output {
        FieldUnion { left: self, right }
    }
}

#[doc(hidden)]
pub trait WireFields: AsRef<[u8]> {
    type Fields;

    const FIELD_COUNT: usize;

    fn fields(&self, base: usize) -> Self::Fields;
    fn field_range(&self, index: usize) -> Option<core::ops::Range<usize>>;
}

/// Entry point for root-relative physical byte selections.
pub struct WireBytes<'view, V> {
    view: &'view V,
}

impl<'view, V> WireBytes<'view, V> {
    #[doc(hidden)]
    #[must_use]
    pub const fn new(view: &'view V) -> Self {
        Self { view }
    }
}

/// Starts a root-relative physical byte selection without reserving a schema method name.
#[must_use]
pub fn select<V: WireFields>(view: &V) -> WireBytes<'_, V> {
    WireBytes::new(view)
}

impl<'view, V: WireFields> WireBytes<'view, V> {
    /// Includes only fields selected by `select`.
    pub fn include<Select, Expr>(self, select: Select) -> Selection<'view, V, Expr, true>
    where
        Select: FnOnce(V::Fields) -> Expr,
        Expr: FieldExpr,
    {
        Selection {
            view: self.view,
            expression: select(self.view.fields(0)),
        }
    }

    /// Excludes fields selected by `select`.
    pub fn exclude<Select, Expr>(self, select: Select) -> Selection<'view, V, Expr, false>
    where
        Select: FnOnce(V::Fields) -> Expr,
        Expr: FieldExpr,
    {
        Selection {
            view: self.view,
            expression: select(self.view.fields(0)),
        }
    }
}
/// Common operation surface accepted by physical-selection callbacks.
pub trait ByteSelection {
    /// Byte iterator borrowed from this selection.
    type Bytes<'selection>: Iterator<Item = u8>
    where
        Self: 'selection;
    /// Chunk iterator borrowed from this selection.
    type Chunks<'selection>: Iterator<Item = &'selection [u8]>
    where
        Self: 'selection;

    /// Iterates selected bytes in physical order.
    fn bytes(&self) -> Self::Bytes<'_>;
    /// Iterates merged selected chunks in physical order.
    fn chunks(&self) -> Self::Chunks<'_>;
    /// Returns the selected byte length.
    fn len(&self) -> usize;

    /// Whether the selection contains no bytes.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Fragmented physical bytes retained without materialization.
pub struct Selection<'view, V, Expr, const INCLUDE: bool> {
    view: &'view V,
    expression: Expr,
}

impl<'view, V, Expr, const INCLUDE: bool> Selection<'view, V, Expr, INCLUDE>
where
    V: WireFields,
    Expr: FieldExpr,
{
    /// Returns selected chunks in physical order with adjacent and overlapping spans merged.
    pub fn chunks(&self) -> SelectionChunks<'_, 'view, V, Expr, INCLUDE> {
        SelectionChunks {
            selection: self,
            index: 0,
            pending: None,
        }
    }

    /// Returns selected bytes in physical order.
    pub fn bytes(&self) -> SelectionBytes<'_, 'view, V, Expr, INCLUDE> {
        SelectionBytes {
            chunks: self.chunks(),
            current: &[],
            offset: 0,
        }
    }

    /// Total selected byte length.
    pub fn len(&self) -> usize {
        self.chunks().map(<[u8]>::len).sum()
    }

    /// Whether no bytes are selected.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<'view, V, Expr, const INCLUDE: bool> ByteSelection for Selection<'view, V, Expr, INCLUDE>
where
    V: WireFields,
    Expr: FieldExpr,
{
    type Bytes<'selection>
        = SelectionBytes<'selection, 'view, V, Expr, INCLUDE>
    where
        Self: 'selection;
    type Chunks<'selection>
        = SelectionChunks<'selection, 'view, V, Expr, INCLUDE>
    where
        Self: 'selection;

    fn bytes(&self) -> Self::Bytes<'_> {
        Selection::bytes(self)
    }

    fn chunks(&self) -> Self::Chunks<'_> {
        Selection::chunks(self)
    }

    fn len(&self) -> usize {
        Selection::len(self)
    }
}

/// Iterator over merged selected chunks.
pub struct SelectionChunks<'selection, 'view, V, Expr, const INCLUDE: bool> {
    selection: &'selection Selection<'view, V, Expr, INCLUDE>,
    index: usize,
    pending: Option<core::ops::Range<usize>>,
}

impl<'selection, 'view, V, Expr, const INCLUDE: bool> Iterator
    for SelectionChunks<'selection, 'view, V, Expr, INCLUDE>
where
    V: WireFields,
    Expr: FieldExpr,
{
    type Item = &'selection [u8];

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < V::FIELD_COUNT {
            let index = self.index;
            self.index += 1;
            let selected = self.selection.expression.contains(index) == INCLUDE;
            if !selected {
                continue;
            }
            let Some(range) = self.selection.view.field_range(index) else {
                continue;
            };
            if range.is_empty() {
                continue;
            }
            match &mut self.pending {
                Some(pending) if range.start <= pending.end => {
                    pending.end = pending.end.max(range.end);
                }
                Some(_) => {
                    let ready = self.pending.replace(range).expect("pending range");
                    return self.selection.view.as_ref().get(ready);
                }
                None => self.pending = Some(range),
            }
        }
        self.pending
            .take()
            .and_then(|range| self.selection.view.as_ref().get(range))
    }
}

/// Iterator over bytes from a fragmented selection.
pub struct SelectionBytes<'selection, 'view, V, Expr, const INCLUDE: bool> {
    chunks: SelectionChunks<'selection, 'view, V, Expr, INCLUDE>,
    current: &'selection [u8],
    offset: usize,
}

impl<'selection, 'view, V, Expr, const INCLUDE: bool> Iterator
    for SelectionBytes<'selection, 'view, V, Expr, INCLUDE>
where
    V: WireFields,
    Expr: FieldExpr,
{
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(value) = self.current.get(self.offset) {
                self.offset += 1;
                return Some(*value);
            }
            self.current = self.chunks.next()?;
            self.offset = 0;
        }
    }
}

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
