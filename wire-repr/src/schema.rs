//! Static schema capabilities used by the schema-only derives.

mod array;
mod selection;
mod sequence;

pub use array::{ArrayError, ArrayItem, ArrayIter, ArrayView, frame_array_extent};
pub use selection::{
    ByteSelection, FieldExpr, FieldPath, FieldPrefix, FieldRoute, FieldRouteEnd, FieldRouteStep,
    FieldUnion, NestedField, Selection, SelectionBytes, SelectionChunks, WireBytes,
    WireFieldSchema, WireFields, select,
};
pub use sequence::{
    Cursor, FixedViews, LeadingWire, NextView, SequenceError, VariableViews, WireSelect,
};

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
/// `State` must be owned and reference-free. Both framing methods must report an extent within
/// `input` and return geometry that remains memory-safe for any immutable span of that exact
/// consumed length. State may retain validated logical values, but it must not make unchecked
/// semantic assumptions about later input bytes.
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not provide the WireView capability",
    label = "this field requires a derived or manual WireView implementation"
)]
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

    /// Whether `frame` can determine one leading representation without consuming the suffix.
    const LEADING_EXTENT: bool = Self::FIXED_SIZE.is_some();

    /// Frames one leading representation when `LEADING_EXTENT` is true.
    ///
    /// Schemas without a leading extent may use the supplied span as an exact boundary.
    fn frame(input: &[u8], absolute_offset: usize) -> Result<Frame<Self::State>, Self::Error>;

    /// Frames an exact supplied span; terminal collection items may remain deferred.
    ///
    /// Override when `frame` must scan those items to delimit a prefix. The default suits
    /// schemas whose leading and exact framing are identical.
    fn frame_exact(
        input: &[u8],
        absolute_offset: usize,
    ) -> Result<Frame<Self::State>, Self::Error> {
        Self::frame(input, absolute_offset)
    }

    /// Reconstructs a child view from geometry established by framing.
    ///
    /// # Safety
    ///
    /// `state` must come from a successful `frame` or `frame_exact` whose consumed length equals
    /// `input.len()`. The address and byte contents need not match the original framed slice.
    unsafe fn from_validated_parts<'view>(
        input: &'view [u8],
        state: &'view Self::State,
    ) -> Self::View<'view>;

    /// Resolves one direct physical field from framing state for typed selections.
    ///
    /// # Safety
    /// `input` and `state` must be the exact span/state pair produced by one successful frame.
    #[doc(hidden)]
    unsafe fn selection_field_range(
        _input: &[u8],
        _state: &Self::State,
        _index: usize,
    ) -> Option<core::ops::Range<usize>> {
        None
    }

    /// Resolves one typed route below a nested physical field.
    ///
    /// # Safety
    /// `input` and `state` must be the exact span/state pair produced by one successful frame.
    /// `Route` must preserve the root witness supplied by the enclosing selection.
    #[doc(hidden)]
    unsafe fn selection_nested_range<Route: FieldRoute>(
        _input: &[u8],
        _state: &Self::State,
        _index: usize,
    ) -> Option<core::ops::Range<usize>> {
        None
    }

    /// Flattens this schema's error when it appears below a recursive repetition boundary.
    ///
    /// Manual capabilities receive a field-site fallback by default. Generated leaf schemas
    /// override this hook to retain standard shortage and extent kinds.
    #[doc(hidden)]
    fn flatten_recursive_error(
        _error: Self::Error,
        fallback_offset: usize,
    ) -> crate::recursive::RecursiveError {
        crate::recursive::RecursiveError::Child {
            offset: fallback_offset,
        }
    }
}

// SAFETY: unit has one empty representation and retains no input-dependent state.
#[allow(unsafe_code)]
unsafe impl WireView for () {
    type Error = core::convert::Infallible;
    type State = ();
    type View<'view> = &'view [u8];

    const FIXED_SIZE: Option<usize> = Some(0);
    const LEADING_EXTENT: bool = true;

    fn frame(_input: &[u8], _absolute_offset: usize) -> Result<Frame<Self::State>, Self::Error> {
        Ok(Frame::new((), 0))
    }

    unsafe fn from_validated_parts<'view>(
        input: &'view [u8],
        _state: &'view Self::State,
    ) -> Self::View<'view> {
        &input[..0]
    }

    fn flatten_recursive_error(
        error: Self::Error,
        _fallback_offset: usize,
    ) -> crate::recursive::RecursiveError {
        match error {}
    }
}

/// Associates a manual or derived schema with its initial builder state.
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not provide the WireBuilder capability",
    label = "this field requires a derived or manual WireBuilder implementation"
)]
pub trait WireBuilder: Sized {
    /// Exact width when every representation has one compile-time width.
    const FIXED_SIZE: Option<usize> = None;

    /// Initial builder type.
    type Builder;

    /// Creates an empty builder.
    fn builder() -> Self::Builder;
}

/// Writes one detached manual or derived builder value into a progressive parent writer.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot write builder value `{V}`",
    label = "this builder value is not accepted by the schema"
)]
pub trait WireWrite<V>: Sized {
    /// Schema-specific write failure.
    type Error: core::error::Error + 'static;

    /// Writes the represented value at the restricted sequential child cursor.
    fn write<O: crate::Output>(
        value: V,
        writer: &mut crate::ChildWriter<'_, O>,
    ) -> Result<(), crate::WriteError<Self::Error, O::GrowError>>;
}

impl WireBuilder for () {
    const FIXED_SIZE: Option<usize> = Some(0);
    type Builder = ();

    fn builder() -> Self::Builder {}
}

impl WireWrite<()> for () {
    type Error = core::convert::Infallible;

    fn write<O: crate::Output>(
        (): (),
        _writer: &mut crate::ChildWriter<'_, O>,
    ) -> Result<(), crate::WriteError<Self::Error, O::GrowError>> {
        Ok(())
    }
}

impl ExactWire<()> for () {
    fn as_wire_bytes(&self) -> &[u8] {
        &[]
    }
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

/// Exact represented bytes known to belong to schema `T`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an exact view of `{T}`",
    label = "exact forwarding requires a validated view of the same schema"
)]
pub trait ExactWire<T> {
    /// Returns the complete represented byte span.
    fn as_wire_bytes(&self) -> &[u8];
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
