use super::WireView;
/// End marker for one statically typed physical field route.
#[doc(hidden)]
pub struct FieldRouteEnd<Root>(core::marker::PhantomData<fn() -> Root>);

/// One field step in a statically typed physical route.
#[doc(hidden)]
pub struct FieldRouteStep<const INDEX: usize, Tail>(core::marker::PhantomData<Tail>);

/// Type-level route prefix used by generated nested field sets.
#[doc(hidden)]
pub trait FieldPrefix {
    type Append<const INDEX: usize>: FieldPrefix;
}

impl<Root> FieldPrefix for FieldRouteEnd<Root> {
    type Append<const INDEX: usize> = FieldRouteStep<INDEX, FieldRouteEnd<Root>>;
}

impl<const HEAD: usize, Tail: FieldPrefix> FieldPrefix for FieldRouteStep<HEAD, Tail> {
    type Append<const INDEX: usize> = FieldRouteStep<HEAD, Tail::Append<INDEX>>;
}

/// Resolves one statically typed physical path against generated schema state.
#[doc(hidden)]
#[allow(unsafe_code)]
pub trait FieldRoute {
    type Root: WireFieldSchema;

    /// # Safety
    /// `input` and `state` must be an exact framed pair for `Schema`.
    unsafe fn resolve<Schema: WireView>(
        input: &[u8],
        state: &Schema::State,
    ) -> Option<core::ops::Range<usize>>;
}

#[doc(hidden)]
pub trait NonEmptyFieldRoute {}

impl<const INDEX: usize, Tail> NonEmptyFieldRoute for FieldRouteStep<INDEX, Tail> {}

#[allow(unsafe_code)]
impl<Root: WireFieldSchema, const INDEX: usize> FieldRoute
    for FieldRouteStep<INDEX, FieldRouteEnd<Root>>
{
    type Root = Root;

    #[inline(always)]
    unsafe fn resolve<Schema: WireView>(
        input: &[u8],
        state: &Schema::State,
    ) -> Option<core::ops::Range<usize>> {
        // SAFETY: the route caller guarantees this exact framed span/state pair.
        unsafe { Schema::selection_field_range(input, state, INDEX) }
    }
}

#[allow(unsafe_code)]
impl<const INDEX: usize, Tail> FieldRoute for FieldRouteStep<INDEX, Tail>
where
    Tail: FieldRoute + NonEmptyFieldRoute,
{
    type Root = Tail::Root;

    #[inline(always)]
    unsafe fn resolve<Schema: WireView>(
        input: &[u8],
        state: &Schema::State,
    ) -> Option<core::ops::Range<usize>> {
        // SAFETY: the route caller guarantees this exact framed span/state pair and root witness.
        unsafe { Schema::selection_nested_range::<Tail>(input, state, INDEX) }
    }
}

/// One generated physical field in a typed selection expression.
pub struct FieldPath<Route>(core::marker::PhantomData<Route>);

impl<Route> Copy for FieldPath<Route> {}

impl<Route> Clone for FieldPath<Route> {
    fn clone(&self) -> Self {
        *self
    }
}

#[allow(unsafe_code)]
impl<Route> FieldPath<Route> {
    #[doc(hidden)]
    /// # Safety
    /// `Route` must originate from the matching generated field-schema family.
    #[must_use]
    pub const unsafe fn new() -> Self {
        Self(core::marker::PhantomData)
    }
}

/// One nested physical field that may be selected whole or descended through generated paths.
pub struct NestedField<Route, Schema>(FieldPath<Route>, core::marker::PhantomData<fn() -> Schema>);

impl<Route, Schema> Copy for NestedField<Route, Schema> {}

impl<Route, Schema> Clone for NestedField<Route, Schema> {
    fn clone(&self) -> Self {
        *self
    }
}

#[allow(unsafe_code)]
impl<Route, Schema> NestedField<Route, Schema> {
    #[doc(hidden)]
    /// # Safety
    /// `Route` must originate from the matching generated field-schema family.
    #[must_use]
    pub const unsafe fn new() -> Self {
        // SAFETY: the caller carries the same route invariant.
        Self(unsafe { FieldPath::new() }, core::marker::PhantomData)
    }

    /// Selects physical fields inside this nested schema.
    pub fn fields<Select, Expr>(self, select: Select) -> Expr
    where
        Route: FieldPrefix,
        Schema: WireFieldSchema,
        Select: FnOnce(Schema::Fields<Route>) -> Expr,
    {
        // SAFETY: this wrapper can only be constructed for the same generated route family.
        select(unsafe { Schema::fields::<Route>() })
    }
}
/// Type-level union of two physical selection expressions.
pub struct FieldUnion<L, R> {
    left: L,
    right: R,
}

#[doc(hidden)]
pub trait FieldExpr<V: WireFields> {
    fn for_each_range<Visit>(&self, view: &V, visit: &mut Visit)
    where
        Visit: FnMut(core::ops::Range<usize>);
}

#[allow(unsafe_code)]
impl<V, Route> FieldExpr<V> for FieldPath<Route>
where
    Route: FieldRoute,
    V: WireFields<SelectionRoot = Route::Root>,
{
    #[inline(always)]
    fn for_each_range<Visit>(&self, view: &V, visit: &mut Visit)
    where
        Visit: FnMut(core::ops::Range<usize>),
    {
        // SAFETY: WireFields certifies that it resolves against its retained exact span/state pair.
        if let Some(range) = unsafe { view.resolve_field_route::<Route>() } {
            visit(range);
        }
    }
}

impl<V, Route, Schema> FieldExpr<V> for NestedField<Route, Schema>
where
    Route: FieldRoute,
    V: WireFields<SelectionRoot = Route::Root>,
{
    #[inline(always)]
    fn for_each_range<Visit>(&self, view: &V, visit: &mut Visit)
    where
        Visit: FnMut(core::ops::Range<usize>),
    {
        self.0.for_each_range(view, visit);
    }
}

impl<V, L, R> FieldExpr<V> for FieldUnion<L, R>
where
    V: WireFields,
    L: FieldExpr<V>,
    R: FieldExpr<V>,
{
    #[inline(always)]
    fn for_each_range<Visit>(&self, view: &V, visit: &mut Visit)
    where
        Visit: FnMut(core::ops::Range<usize>),
    {
        self.left.for_each_range(view, visit);
        self.right.for_each_range(view, visit);
    }
}

impl<Route, R> core::ops::BitOr<R> for FieldPath<Route> {
    type Output = FieldUnion<Self, R>;

    fn bitor(self, right: R) -> Self::Output {
        FieldUnion { left: self, right }
    }
}

impl<Route, Schema, R> core::ops::BitOr<R> for NestedField<Route, Schema> {
    type Output = FieldUnion<Self, R>;

    fn bitor(self, right: R) -> Self::Output {
        FieldUnion { left: self, right }
    }
}

impl<L, R, Next> core::ops::BitOr<Next> for FieldUnion<L, R> {
    type Output = FieldUnion<Self, Next>;

    fn bitor(self, right: Next) -> Self::Output {
        FieldUnion { left: self, right }
    }
}

/// View-side owner of one exact typed field family.
///
/// # Safety
///
/// `SelectionRoot` must be the schema that framed this view. `resolve_field_route` must pass this
/// view's exact represented span with its matching retained state to `FieldRoute::resolve`, and
/// every path returned by `fields` must resolve through that same root. An empty field family may
/// return `None` for every route.
#[doc(hidden)]
#[allow(unsafe_code)]
pub unsafe trait WireFields: AsRef<[u8]> {
    type Fields;
    type SelectionRoot: WireFieldSchema;

    fn fields(&self) -> Self::Fields;
    fn field_range(&self, index: usize) -> Option<core::ops::Range<usize>>;

    /// # Safety
    /// The implementor must pass this view's exact represented span and matching retained state.
    #[doc(hidden)]
    unsafe fn resolve_field_route<Route>(&self) -> Option<core::ops::Range<usize>>
    where
        Route: FieldRoute<Root = Self::SelectionRoot>;
}

/// Generated or manual schema-side family of typed physical field routes and range resolution.
///
/// # Safety
///
/// Every path returned by `fields::<Prefix>` must preserve `Prefix` and append only this schema's
/// physical field ordinals. The corresponding `WireView::selection_field_range` hooks must return
/// exact ranges relative to their supplied input, while `selection_nested_range` must translate a
/// child route without changing its root witness. A manual schema that exposes no nested paths may
/// use an empty field family and return `None` from both hooks.
#[doc(hidden)]
#[allow(unsafe_code)]
pub unsafe trait WireFieldSchema: WireView {
    type Fields<Prefix: FieldPrefix>;

    /// # Safety
    /// `Prefix` must originate from this schema's generated root or a validated nested field path.
    unsafe fn fields<Prefix: FieldPrefix>() -> Self::Fields<Prefix>;
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
        Expr: FieldExpr<V>,
    {
        Selection {
            view: self.view,
            expression: select(self.view.fields()),
        }
    }

    /// Excludes fields selected by `select`.
    pub fn exclude<Select, Expr>(self, select: Select) -> Selection<'view, V, Expr, false>
    where
        Select: FnOnce(V::Fields) -> Expr,
        Expr: FieldExpr<V>,
    {
        Selection {
            view: self.view,
            expression: select(self.view.fields()),
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
    Expr: FieldExpr<V>,
{
    /// Returns selected chunks in physical order with adjacent and overlapping spans merged.
    pub fn chunks(&self) -> SelectionChunks<'_, 'view, V, Expr, INCLUDE> {
        SelectionChunks {
            selection: self,
            cursor: 0,
            done: false,
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
    Expr: FieldExpr<V>,
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
    cursor: usize,
    done: bool,
}

impl<'selection, 'view, V, Expr, const INCLUDE: bool>
    SelectionChunks<'selection, 'view, V, Expr, INCLUDE>
where
    V: WireFields,
    Expr: FieldExpr<V>,
{
    fn for_each_range(&self, mut visit: impl FnMut(core::ops::Range<usize>)) {
        let input_len = self.selection.view.as_ref().len();
        self.selection
            .expression
            .for_each_range(self.selection.view, &mut |range| {
                if range.start < range.end && range.end <= input_len {
                    visit(range);
                }
            });
    }

    fn next_included(&mut self) -> Option<core::ops::Range<usize>> {
        let mut start = usize::MAX;
        self.for_each_range(|range| {
            if range.end > self.cursor {
                start = start.min(range.start.max(self.cursor));
            }
        });
        if start == usize::MAX {
            return None;
        }
        let mut end = start;
        loop {
            let previous = end;
            self.for_each_range(|range| {
                if range.start <= end && range.end > start {
                    end = end.max(range.end);
                }
            });
            if end == previous {
                break;
            }
        }
        self.cursor = end;
        Some(start..end)
    }

    fn next_excluded(&mut self) -> Option<core::ops::Range<usize>> {
        let input_len = self.selection.view.as_ref().len();
        loop {
            if self.cursor >= input_len {
                return None;
            }
            let mut covered_end = self.cursor;
            loop {
                let previous = covered_end;
                self.for_each_range(|range| {
                    if range.start <= covered_end && range.end > self.cursor {
                        covered_end = covered_end.max(range.end);
                    }
                });
                if covered_end == previous {
                    break;
                }
            }
            if covered_end > self.cursor {
                self.cursor = covered_end;
                continue;
            }
            let mut next_start = input_len;
            self.for_each_range(|range| {
                if range.start > self.cursor {
                    next_start = next_start.min(range.start);
                }
            });
            let ready = self.cursor..next_start;
            self.cursor = next_start;
            return Some(ready);
        }
    }
}

impl<'selection, 'view, V, Expr, const INCLUDE: bool> Iterator
    for SelectionChunks<'selection, 'view, V, Expr, INCLUDE>
where
    V: WireFields,
    Expr: FieldExpr<V>,
{
    type Item = &'selection [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let range = if INCLUDE {
            self.next_included()
        } else {
            self.next_excluded()
        };
        let Some(range) = range else {
            self.done = true;
            return None;
        };
        self.selection.view.as_ref().get(range)
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
    Expr: FieldExpr<V>,
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
