//! Static byte-range selection over prepared byte sources.

use core::ops::{BitOr, Range};

use crate::{ByteSegment, ByteSink, ByteSource, ByteSourceCursor};

/// Describes one or more absolute field byte ranges for a concrete source type.
///
/// This trait is intended for derive-generated zero-sized field markers. Ranges are
/// half-open spans in the physical representation emitted by `T`.
#[doc(hidden)]
pub trait FieldSelection<T: ?Sized> {
    /// Visits every field representation span selected by this marker.
    fn visit_ranges<V>(&self, target: &T, visitor: &mut V)
    where
        V: FnMut(Range<usize>);

    /// Returns a generated-marker byte count when it can emit directly.
    #[doc(hidden)]
    fn direct_len(&self, _target: &T) -> Option<usize> {
        None
    }

    /// Emits a generated-marker selection directly, returning whether it handled the source.
    #[doc(hidden)]
    fn emit_direct<S: ByteSink>(&self, _target: &T, _sink: &mut S) -> bool {
        false
    }
}

/// Selects a generated field by directly borrowing its prepared byte source.
///
/// This is intentionally narrower than [`FieldSelection`]: unions and arbitrary
/// selections still use range selection to preserve physical wire order and deduplicate
/// overlapping ranges.
#[doc(hidden)]
pub trait DirectFieldSelection<T: ?Sized> {
    /// The directly selected byte source.
    type Source<'a>: ByteSourceCursor
    where
        T: 'a;

    /// Borrows the selected prepared field source.
    fn direct_source<'a>(&self, target: &'a T) -> Self::Source<'a>;
}

/// A recursive union of two field selections.
#[doc(hidden)]
pub struct FieldUnion<L, R> {
    left: L,
    right: R,
}

impl<L, R> FieldUnion<L, R> {
    /// Combines two selections without runtime descriptor storage.
    #[must_use]
    pub const fn new(left: L, right: R) -> Self {
        Self { left, right }
    }
}

impl<T: ?Sized, L, R> FieldSelection<T> for FieldUnion<L, R>
where
    L: FieldSelection<T>,
    R: FieldSelection<T>,
{
    #[inline(always)]
    fn visit_ranges<V>(&self, target: &T, visitor: &mut V)
    where
        V: FnMut(Range<usize>),
    {
        self.left.visit_ranges(target, visitor);
        self.right.visit_ranges(target, visitor);
    }
}

impl<L, R, Right> BitOr<Right> for FieldUnion<L, R> {
    type Output = FieldUnion<Self, Right>;

    fn bitor(self, right: Right) -> Self::Output {
        FieldUnion::new(self, right)
    }
}

/// Wraps local generated markers so they resolve against a prepared-plan ancestor.
#[doc(hidden)]
pub trait MarkerScope {
    /// The marker representation after applying this scope.
    type Wrap<M: Copy>: Copy;

    /// Wraps a local marker in this scope.
    fn wrap<M: Copy>(marker: M) -> Self::Wrap<M>;
}

/// The scope used by a prepared plan's root field proxy.
#[doc(hidden)]
pub struct RootScope;

impl MarkerScope for RootScope {
    type Wrap<M: Copy> = M;

    fn wrap<M: Copy>(marker: M) -> M {
        marker
    }
}

/// Composes a local prepared-plan projection with an enclosing marker scope.
#[doc(hidden)]
pub struct Through<Outer, Projection>(core::marker::PhantomData<fn() -> (Outer, Projection)>);

impl<Outer, Projection> Copy for Through<Outer, Projection> {}

impl<Outer, Projection> Clone for Through<Outer, Projection> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Outer: MarkerScope, Projection: Copy> MarkerScope for Through<Outer, Projection> {
    type Wrap<M: Copy> = Outer::Wrap<Translated<Projection, M>>;

    fn wrap<M: Copy>(marker: M) -> Self::Wrap<M> {
        Outer::wrap(Translated::new(marker))
    }
}

/// Projects a direct nested field from its enclosing byte source.
#[doc(hidden)]
pub trait FieldProjection<Outer: ?Sized> {
    /// The child byte-source type stored in the projected field.
    type Inner: ?Sized;

    /// Returns the child field's outer-coordinate span and stored byte source.
    #[allow(clippy::needless_lifetimes)]
    fn project<'a>(outer: &'a Outer) -> (Range<usize>, &'a Self::Inner);
}

/// Projects a nested source for direct cursor selection.
#[doc(hidden)]
pub trait DirectFieldProjection<Outer: ?Sized> {
    /// The child source borrowed for a given outer-source lifetime.
    type Inner<'a>: ?Sized + 'a
    where
        Outer: 'a;

    /// Returns the child source without translating it into outer coordinates.
    fn direct_project<'a>(outer: &'a Outer) -> &'a Self::Inner<'a>;
}

/// A child-plan selection translated into an enclosing prepared-plan coordinate space.
#[doc(hidden)]
pub struct Translated<Projection, Selection> {
    selection: Selection,
    projection: core::marker::PhantomData<fn() -> Projection>,
}

impl<Projection, Selection> Translated<Projection, Selection> {
    /// Creates a translated child selection.
    #[must_use]
    pub const fn new(selection: Selection) -> Self {
        Self {
            selection,
            projection: core::marker::PhantomData,
        }
    }
}

impl<Projection, Selection: Copy> Copy for Translated<Projection, Selection> {}

impl<Projection, Selection: Copy> Clone for Translated<Projection, Selection> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Outer: ByteSource + ?Sized, Projection, Selection> FieldSelection<Outer>
    for Translated<Projection, Selection>
where
    Projection: FieldProjection<Outer>,
    Selection: FieldSelection<Projection::Inner>,
{
    #[inline(always)]
    fn visit_ranges<V>(&self, outer: &Outer, visitor: &mut V)
    where
        V: FnMut(Range<usize>),
    {
        let (span, child) = Projection::project(outer);
        self.selection.visit_ranges(child, &mut |range| {
            let start = span
                .start
                .checked_add(range.start)
                .expect("translated field range overflow");
            let end = span
                .start
                .checked_add(range.end)
                .expect("translated field range overflow");
            assert!(
                start <= end && end <= span.end,
                "nested field selection escaped its parent span"
            );
            visitor(start..end);
        });
    }

    #[inline(always)]
    fn direct_len(&self, outer: &Outer) -> Option<usize> {
        let (span, child) = Projection::project(outer);
        let selected_len = self.selection.direct_len(child)?;
        assert!(
            span.start <= span.end && span.end <= outer.byte_len(),
            "nested field projection is outside the source"
        );
        assert!(
            selected_len <= span.end - span.start,
            "nested direct selection escaped its parent span"
        );
        Some(selected_len)
    }

    #[inline(always)]
    fn emit_direct<S: ByteSink>(&self, outer: &Outer, sink: &mut S) -> bool {
        let (span, child) = Projection::project(outer);
        let Some(selected_len) = self.selection.direct_len(child) else {
            return false;
        };
        assert!(
            span.start <= span.end && span.end <= outer.byte_len(),
            "nested field projection is outside the source"
        );
        assert!(
            selected_len <= span.end - span.start,
            "nested direct selection escaped its parent span"
        );
        self.selection.emit_direct(child, sink)
    }
}

impl<Outer: ?Sized, Projection, Selection> DirectFieldSelection<Outer>
    for Translated<Projection, Selection>
where
    Projection: DirectFieldProjection<Outer>,
    for<'a> Selection: DirectFieldSelection<Projection::Inner<'a>>,
{
    type Source<'a>
        = <Selection as DirectFieldSelection<Projection::Inner<'a>>>::Source<'a>
    where
        Outer: 'a;

    #[inline(always)]
    fn direct_source<'a>(&self, outer: &'a Outer) -> Self::Source<'a> {
        let child = Projection::direct_project(outer);
        <Selection as DirectFieldSelection<Projection::Inner<'a>>>::direct_source(
            &self.selection,
            child,
        )
    }
}

impl<Projection, Selection, Right> BitOr<Right> for Translated<Projection, Selection> {
    type Output = FieldUnion<Self, Right>;

    fn bitor(self, right: Right) -> Self::Output {
        FieldUnion::new(self, right)
    }
}

/// A nested proxy field that selects its whole child or dereferences to child members.
#[doc(hidden)]
pub struct NestedField<Whole, Fields> {
    whole: Whole,
    fields: Fields,
}

impl<Whole, Fields> NestedField<Whole, Fields> {
    /// Creates a nested field proxy.
    #[must_use]
    pub const fn new(whole: Whole, fields: Fields) -> Self {
        Self { whole, fields }
    }
}

impl<Whole: Copy, Fields: Copy> Copy for NestedField<Whole, Fields> {}

impl<Whole: Copy, Fields: Copy> Clone for NestedField<Whole, Fields> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Whole, Fields> core::ops::Deref for NestedField<Whole, Fields> {
    type Target = Fields;

    fn deref(&self) -> &Fields {
        &self.fields
    }
}

impl<T: ?Sized, Whole, Fields> FieldSelection<T> for NestedField<Whole, Fields>
where
    Whole: FieldSelection<T>,
{
    #[inline(always)]
    fn visit_ranges<V>(&self, target: &T, visitor: &mut V)
    where
        V: FnMut(Range<usize>),
    {
        self.whole.visit_ranges(target, visitor);
    }
}

impl<T: ?Sized, Whole, Fields> DirectFieldSelection<T> for NestedField<Whole, Fields>
where
    Whole: DirectFieldSelection<T>,
{
    type Source<'a>
        = Whole::Source<'a>
    where
        T: 'a;

    #[inline(always)]
    fn direct_source<'a>(&self, target: &'a T) -> Self::Source<'a> {
        self.whole.direct_source(target)
    }
}

impl<Whole, Fields, Right> BitOr<Right> for NestedField<Whole, Fields> {
    type Output = FieldUnion<Self, Right>;

    fn bitor(self, right: Right) -> Self::Output {
        FieldUnion::new(self, right)
    }
}

/// A source together with its generated zero-sized field proxy.
pub struct ByteSelection<'source, T, Fields> {
    source: &'source T,
    fields: Fields,
}

impl<'source, T, Fields> ByteSelection<'source, T, Fields> {
    /// Wraps a prepared source and its field proxy.
    #[must_use]
    pub const fn new(source: &'source T, fields: Fields) -> Self {
        Self { source, fields }
    }

    /// Returns the complete source length.
    #[must_use]
    pub fn byte_len(&self) -> usize
    where
        T: ByteSource,
    {
        self.source.byte_len()
    }

    /// Streams the complete source while checking its declared length.
    pub fn emit_to<S: ByteSink>(&self, sink: &mut S)
    where
        T: ByteSource,
    {
        emit_all(self.source, sink);
    }

    /// Writes the complete source into an exactly-sized output slice.
    pub fn write_into(&self, output: &mut [u8])
    where
        T: ByteSource,
    {
        ByteSource::write_into(self, output);
    }

    /// Selects the exact representation bytes of the chosen fields.
    #[must_use]
    #[inline(always)]
    pub fn include<Selection>(
        &self,
        choose: impl FnOnce(&Fields) -> Selection,
    ) -> IncludedBytes<'source, T, Selection>
    where
        T: ByteSource,
        Selection: FieldSelection<T>,
    {
        IncludedBytes::new(self.source, choose(&self.fields))
    }

    /// Borrows the prepared source of one directly selected generated field.
    #[must_use]
    #[inline(always)]
    pub fn include_direct<Selection>(
        &self,
        choose: impl FnOnce(&Fields) -> Selection,
    ) -> Selection::Source<'source>
    where
        Selection: DirectFieldSelection<T>,
    {
        choose(&self.fields).direct_source(self.source)
    }

    /// Selects every source byte except the exact representation bytes of the chosen fields.
    #[must_use]
    #[inline(always)]
    pub fn exclude<Selection>(
        &self,
        choose: impl FnOnce(&Fields) -> Selection,
    ) -> ExcludedBytes<'source, T, Selection>
    where
        T: ByteSource,
        Selection: FieldSelection<T>,
    {
        ExcludedBytes::new(self.source, choose(&self.fields))
    }
}

impl<T: ByteSource, Fields> ByteSource for ByteSelection<'_, T, Fields> {
    fn byte_len(&self) -> usize {
        self.byte_len()
    }

    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        self.emit_to(sink);
    }
}

impl<T: ByteSourceCursor, Fields> ByteSourceCursor for ByteSelection<'_, T, Fields> {
    type Segments<'source>
        = T::Segments<'source>
    where
        Self: 'source;

    #[inline(always)]
    fn segments(&self) -> Self::Segments<'_> {
        self.source.segments()
    }

    type Bytes<'source>
        = T::Bytes<'source>
    where
        Self: 'source;

    #[inline(always)]
    fn bytes(&self) -> Self::Bytes<'_> {
        self.source.bytes()
    }
}

/// The included bytes of a static field selection.
pub struct IncludedBytes<'source, T, Selection> {
    source: &'source T,
    selection: Selection,
    byte_len: usize,
}

impl<'source, T: ByteSource, Selection: FieldSelection<T>> IncludedBytes<'source, T, Selection> {
    #[inline(always)]
    fn new(source: &'source T, selection: Selection) -> Self {
        let byte_len = selection
            .direct_len(source)
            .unwrap_or_else(|| selected_len(source, &selection, true));
        Self {
            source,
            selection,
            byte_len,
        }
    }

    /// Returns the exact number of included bytes.
    #[must_use]
    pub const fn byte_len(&self) -> usize {
        self.byte_len
    }

    /// Streams included bytes in their original wire order.
    #[inline(always)]
    pub fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        if !self.selection.emit_direct(self.source, sink) {
            emit_selected(self.source, &self.selection, self.byte_len, true, sink);
        }
    }

    /// Writes included bytes into an exactly-sized output slice.
    pub fn write_into(&self, output: &mut [u8]) {
        ByteSource::write_into(self, output);
    }
}

impl<T: ByteSource, Selection: FieldSelection<T>> ByteSource for IncludedBytes<'_, T, Selection> {
    #[inline(always)]
    fn byte_len(&self) -> usize {
        self.byte_len()
    }

    #[inline(always)]
    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        self.emit_to(sink);
    }
}

impl<T: ByteSourceCursor, Selection: FieldSelection<T>> ByteSourceCursor
    for IncludedBytes<'_, T, Selection>
{
    type Segments<'source>
        = SelectedSegments<'source, T::Segments<'source>, T, Selection>
    where
        Self: 'source;

    #[inline(always)]
    fn segments(&self) -> Self::Segments<'_> {
        SelectedSegments::new(self.source, &self.selection, true)
    }

    type Bytes<'source>
        = crate::ByteBytes<'source, SelectedSegments<'source, T::Segments<'source>, T, Selection>>
    where
        Self: 'source;

    #[inline(always)]
    fn bytes(&self) -> Self::Bytes<'_> {
        crate::ByteBytes::new(self.segments())
    }
}

/// The bytes outside a static field selection.
pub struct ExcludedBytes<'source, T, Selection> {
    source: &'source T,
    selection: Selection,
    byte_len: usize,
}

impl<'source, T: ByteSource, Selection: FieldSelection<T>> ExcludedBytes<'source, T, Selection> {
    #[inline(always)]
    fn new(source: &'source T, selection: Selection) -> Self {
        let byte_len = selected_len(source, &selection, false);
        Self {
            source,
            selection,
            byte_len,
        }
    }

    /// Returns the exact number of excluded bytes.
    #[must_use]
    pub const fn byte_len(&self) -> usize {
        self.byte_len
    }

    /// Streams excluded bytes in their original wire order.
    #[inline(always)]
    pub fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        emit_selected(self.source, &self.selection, self.byte_len, false, sink);
    }

    /// Writes excluded bytes into an exactly-sized output slice.
    pub fn write_into(&self, output: &mut [u8]) {
        ByteSource::write_into(self, output);
    }
}

impl<T: ByteSource, Selection: FieldSelection<T>> ByteSource for ExcludedBytes<'_, T, Selection> {
    #[inline(always)]
    fn byte_len(&self) -> usize {
        self.byte_len()
    }

    #[inline(always)]
    fn emit_to<S: ByteSink>(&self, sink: &mut S) {
        self.emit_to(sink);
    }
}

impl<T: ByteSourceCursor, Selection: FieldSelection<T>> ByteSourceCursor
    for ExcludedBytes<'_, T, Selection>
{
    type Segments<'source>
        = SelectedSegments<'source, T::Segments<'source>, T, Selection>
    where
        Self: 'source;

    #[inline(always)]
    fn segments(&self) -> Self::Segments<'_> {
        SelectedSegments::new(self.source, &self.selection, false)
    }

    type Bytes<'source>
        = crate::ByteBytes<'source, SelectedSegments<'source, T::Segments<'source>, T, Selection>>
    where
        Self: 'source;

    #[inline(always)]
    fn bytes(&self) -> Self::Bytes<'_> {
        crate::ByteBytes::new(self.segments())
    }
}

/// A static cursor over included or excluded spans of another byte source.
pub struct SelectedSegments<'source, I, T: ?Sized, Selection> {
    source: &'source T,
    selection: &'source Selection,
    include: bool,
    segments: I,
    current: Option<ByteSegment<'source>>,
    source_position: usize,
    segment_offset: usize,
}

impl<'source, T, Selection> SelectedSegments<'source, T::Segments<'source>, T, Selection>
where
    T: ByteSourceCursor + ?Sized,
    Selection: FieldSelection<T>,
{
    #[inline(always)]
    fn new(source: &'source T, selection: &'source Selection, include: bool) -> Self {
        validate_ranges(source, selection);
        Self {
            source,
            selection,
            include,
            segments: source.segments(),
            current: None,
            source_position: 0,
            segment_offset: 0,
        }
    }
}

impl<'source, I, T, Selection> Iterator for SelectedSegments<'source, I, T, Selection>
where
    I: Iterator<Item = ByteSegment<'source>>,
    T: ByteSource + ?Sized,
    Selection: FieldSelection<T>,
{
    type Item = ByteSegment<'source>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let segment = match self.current {
                Some(segment) => segment,
                None => {
                    let segment = self.segments.next()?;
                    self.current = Some(segment);
                    self.segment_offset = 0;
                    segment
                }
            };
            let segment_len = segment.len();
            if self.segment_offset == segment_len {
                self.source_position = self
                    .source_position
                    .checked_add(segment_len)
                    .expect("byte source position overflow");
                self.current = None;
                continue;
            }

            let position = self
                .source_position
                .checked_add(self.segment_offset)
                .expect("byte source position overflow");
            let (covered, boundary) = coverage_at(self.source, self.selection, position);
            let segment_end = self
                .source_position
                .checked_add(segment_len)
                .expect("byte source position overflow");
            let selected = covered == self.include;
            let mut end = boundary.min(segment_end);
            while selected && end < segment_end {
                let (covered, boundary) = coverage_at(self.source, self.selection, end);
                if covered != self.include {
                    break;
                }
                end = boundary.min(segment_end);
            }
            let start = self.segment_offset;
            let len = end - position;
            self.segment_offset += len;
            if selected {
                return Some(segment.subsegment(start, len));
            }
        }
    }
}

#[inline(always)]
fn selected_len<T: ByteSource + ?Sized, Selection: FieldSelection<T>>(
    source: &T,
    selection: &Selection,
    include: bool,
) -> usize {
    let source_len = source.byte_len();
    validate_ranges(source, selection);
    let mut position = 0;
    let mut total: usize = 0;
    while position < source_len {
        let (covered, boundary) = coverage_at(source, selection, position);
        let selected = if include { covered } else { !covered };
        if selected {
            total = total
                .checked_add(boundary - position)
                .expect("selected byte length overflow");
        }
        position = boundary;
    }
    total
}

#[inline(always)]
fn validate_ranges<T: ByteSource + ?Sized, Selection: FieldSelection<T>>(
    source: &T,
    selection: &Selection,
) {
    let source_len = source.byte_len();
    selection.visit_ranges(source, &mut |range| {
        assert!(
            range.start <= range.end && range.end <= source_len,
            "field selection range is outside the source"
        );
    });
}

#[inline(always)]
fn coverage_at<T: ByteSource + ?Sized, Selection: FieldSelection<T>>(
    source: &T,
    selection: &Selection,
    position: usize,
) -> (bool, usize) {
    let source_len = source.byte_len();
    let mut covered = false;
    let mut boundary = source_len;
    selection.visit_ranges(source, &mut |range| {
        assert!(
            range.start <= range.end && range.end <= source_len,
            "field selection range is outside the source"
        );
        if range.start <= position && position < range.end {
            covered = true;
            boundary = boundary.min(range.end);
        } else if position < range.start {
            boundary = boundary.min(range.start);
        }
    });
    assert!(boundary > position, "field selection made no progress");
    (covered, boundary)
}

fn emit_all<T: ByteSource, S: ByteSink>(source: &T, sink: &mut S) {
    let mut checking = CountingSink {
        sink,
        source_len: source.byte_len(),
        position: 0,
    };
    source.emit_to(&mut checking);
    checking.finish();
}

#[inline(always)]
fn emit_selected<T: ByteSource, Selection: FieldSelection<T>, S: ByteSink>(
    source: &T,
    selection: &Selection,
    selected_len: usize,
    include: bool,
    sink: &mut S,
) {
    let source_len = source.byte_len();
    validate_ranges(source, selection);
    let mut filtering = FilteringSink {
        sink,
        target: source,
        selection,
        source_len,
        selected_len,
        source_position: 0,
        selected_position: 0,
        include,
    };
    source.emit_to(&mut filtering);
    filtering.finish();
}

struct CountingSink<'sink, S> {
    sink: &'sink mut S,
    source_len: usize,
    position: usize,
}

impl<S: ByteSink> CountingSink<'_, S> {
    fn consume(&mut self, len: usize) {
        self.position = self
            .position
            .checked_add(len)
            .expect("ByteSource emission length overflow");
        assert!(
            self.position <= self.source_len,
            "ByteSource emitted too many bytes"
        );
    }

    fn finish(&self) {
        assert_eq!(
            self.position, self.source_len,
            "ByteSource emitted wrong length"
        );
    }
}

impl<S: ByteSink> ByteSink for CountingSink<'_, S> {
    fn write(&mut self, bytes: &[u8]) {
        self.consume(bytes.len());
        self.sink.write(bytes);
    }

    fn fill(&mut self, byte: u8, len: usize) {
        self.consume(len);
        self.sink.fill(byte, len);
    }
}

struct FilteringSink<'sink, 'target, 'selection, S, T: ?Sized, Selection> {
    sink: &'sink mut S,
    target: &'target T,
    selection: &'selection Selection,
    source_len: usize,
    selected_len: usize,
    source_position: usize,
    selected_position: usize,
    include: bool,
}

impl<S: ByteSink, T: ByteSource + ?Sized, Selection: FieldSelection<T>>
    FilteringSink<'_, '_, '_, S, T, Selection>
{
    #[inline(always)]
    fn consume(&mut self, len: usize, mut emit: impl FnMut(&mut S, usize, usize)) {
        let end = self
            .source_position
            .checked_add(len)
            .expect("ByteSource emission length overflow");
        assert!(end <= self.source_len, "ByteSource emitted too many bytes");

        while self.source_position < end {
            let (covered, boundary) =
                coverage_at(self.target, self.selection, self.source_position);
            let segment_end = boundary.min(end);
            let selected = if self.include { covered } else { !covered };
            if selected {
                let selected_count = segment_end - self.source_position;
                self.selected_position = self
                    .selected_position
                    .checked_add(selected_count)
                    .expect("selected byte length overflow");
                assert!(
                    self.selected_position <= self.selected_len,
                    "field selection emitted too many bytes"
                );
                emit(self.sink, self.source_position, selected_count);
            }
            self.source_position = segment_end;
        }
    }

    #[inline(always)]
    fn finish(&self) {
        assert_eq!(
            self.source_position, self.source_len,
            "ByteSource emitted wrong length"
        );
        assert_eq!(
            self.selected_position, self.selected_len,
            "field selection emitted wrong length"
        );
    }
}

impl<S: ByteSink, T: ByteSource + ?Sized, Selection: FieldSelection<T>> ByteSink
    for FilteringSink<'_, '_, '_, S, T, Selection>
{
    #[inline(always)]
    fn write(&mut self, bytes: &[u8]) {
        let chunk_start = self.source_position;
        self.consume(bytes.len(), |sink, position, len| {
            let start = position - chunk_start;
            sink.write(&bytes[start..start + len]);
        });
    }

    #[inline(always)]
    fn fill(&mut self, byte: u8, len: usize) {
        self.consume(len, |sink, _, selected_len| sink.fill(byte, selected_len));
    }
}
