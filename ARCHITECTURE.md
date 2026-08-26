# Architecture

This document defines the target production architecture and clean-cutover contract for
`wire-repr`. The current implementation covers named structs, fixed scalars and byte arrays,
constants, explicit logical conversions, validators, nested children, demand geometry, controller
dependencies, conditional choice groups, counted runtime arrays, static enums, nominal and inline
bitfields, root-relative physical selections, computed fields, homogeneous views, heterogeneous
cursors, and exact View forwarding. Nested selection paths remain future composition work alongside
the separately deferred traversal and recursive-layout design. New layout classes extend this one
model rather than restoring the removed legacy renderer or introducing parallel modes.

## 1. Product boundary

`wire-repr` compiles Rust schema declarations into exact-source views and progressive writers.
A schema struct or enum describes a physical representation. It is not a semantic value and is
never constructed as the decoded object.

```rust
#[derive(WireView, WireBuilder)]
struct InvokeWithLayer<Q> {
    #[wire(le, constant = 0xda9b_0d0d)]
    constructor: u32,
    #[wire(le)]
    layer: i32,
    query: Q,
}
```

The two capabilities are independent:

- `WireView` derives retained, validated read views;
- `WireBuilder` derives progressive typestate writers.

A schema may derive either or both. The final API has no `derive(Wire)` shorthand and no legacy
feature. The public API is safe, `no_std`, allocation-free, and featureless. A narrow audited
unsafe bridge reconstructs nested views from descriptor state already proven by framing.

Consumers own protocol and application semantics. The library owns physical widths, byte order,
field order, tags, constants, dynamic extents, conditional presence, placement, framing, and
complete represented ranges.

## 2. Views and backing ownership

Exact reading has one entry point:

```rust
Schema::view<T: AsRef<[u8]>>(input: T) -> Result<impl SchemaView, Error>
```

The generated hidden state stores `T` directly:

- `view(&bytes[..])` borrows;
- `view(&vec)` borrows;
- `view(vec)` owns the `Vec`;
- `view(bytes::Bytes)` owns the shared handle.

Ownership follows ordinary Rust argument ownership. There is no `bytes` feature and no second
renderer. `bytes::Bytes` is only a dev-dependency used to prove that arbitrary owned `AsRef`
backing works while wire-repr features are disabled.

Retained backing must project the same immutable byte span while the view exists. This is the
backing contract used by slices, `Vec`, `bytes::Bytes`, and ordinary owned wrappers. Stateful
`AsRef` implementations that switch or mutate their projection are outside the API contract.

A public generated view trait is lifetime-free. The opaque concrete type captures the backing and
its lifetime. Nested getters return small views borrowing their parent. A getter is infallible when
the parent can prove its range cheaply; deferred dynamic getters return `Result` and frame only the
requested range. Nested views do not clone or slice owned backing into independent handles.

`view()` treats the supplied span as one exact representation and proves root-local geometry.
Declared outer extents must match that span. It does not traverse untouched nested values or
collections merely to validate them. Explicit schema validators run only because the schema author
requested that domain check; `view_unchecked()` skips them without weakening local memory safety.

## 3. Framing and descriptors

Framing establishes only the geometry required to locate and safely expose the requested fields.
Values remain lazy. Fixed scalar getters decode from exact source bytes when called.

Every schema has a static read capability with:

- a lifetime-free typed error;
- a reference-free `State: 'static` descriptor;
- a borrowed nested-view family;
- fixed-width and leading-extent metadata when provable.

Top-level view state owns the descriptor. A nested child view borrows its input span and the
descriptor state already proven for that span. Descriptors never contain input references,
self-references, runtime schema objects, dynamic dispatch, or per-item indexes.

Generated descriptors and getters specialize by geometry:

- fixed and suffix-derived offsets occupy no state;
- length-bounded and terminal children expose direct ranges;
- homogeneous prefix grammars use counters;
- sequential dynamic geometry retains only endpoints actually discovered;
- a dependent getter may replay earlier variable geometry instead of caching an index;
- controller values are retained only when later geometry needs them.

General recursive schemas and a public traversal capability are deferred beyond this roadmap. They
are not prerequisites for demand-framed fields, runtime collections, enums, selections, `views`, or
cursors, and will be designed independently after those representation classes are complete.

The current fixed-layout vertical cannot inspect a generic child's associated fixed-size capability
during macro expansion. Instantiating a nonterminal child whose `FIXED_SIZE` is `None` therefore
returns a field-site `LayoutUnavailable`/`LayoutError` from framing or writing; it never panics.
The demand-geometry vertical replaces that capability error with lazy endpoint discovery.

Manual `WireView` implementations use the same contract and are `unsafe impl`s. They explicitly
separate `frame` from `from_validated_parts` and certify that owned, reference-free state remains
memory-safe for any immutable span of the framed length. Validated logical values are retained in
state or safely revalidated; generated parents check child extents before reconstruction.

Safe Rust cannot turn previously validated bytes into `&str` without checking UTF-8 again.
A textual getter therefore revalidates UTF-8 or returns bytes. It does not reparse field geometry.

## 4. Rust generics

Schema derives preserve ordinary lifetime, type, const, and `where` generics. The macro adds only
bounds required by the selected capability.

A generic field accepts any type implementing the required public wire capability: builtin,
manual, or derived. A foreign type without that capability requires a local wrapper or a manual
implementation.
Generated descriptors are monomorphized and use no trait objects or runtime schema reflection.
Generic child `State` is embedded in the parent read descriptor; generic child writers share the
parent output cursor.

Generated internal generic names are fresh and cannot collide with user parameters. Lifetimes
remain first in generated generic declarations.

## 5. Physical fields

Declaration order is physical order. The target field vocabulary includes:

- fixed Rust integers and floats with explicit `be` or `le` where width exceeds one byte;
- `usize`, `isize`, `bool`, and `char` with an explicit physical `as = integer_type`;
- fixed byte arrays;
- general stored constants with `constant = value`;
- manual and derived nested wire types;
- `wire::Bytes` for lifetime-free variable raw bytes in schema declarations;
- bounded raw or nested representations controlled by `bytes = path`;
- runtime arrays represented by `wire::Array<T>` and `counted_by = path`;
- terminal `rest` bytes;
- conditional fields with `depends_on = bool_path`;
- zero-width logical flags with `flag = bool_path`;
- bitfields, padding, alignment, and forward placement;
- computed stored fields.

Controllers remain real physical fields. They are not hidden inside convenience container types:

```rust
#[wire(le)]
count: u32,
#[wire(counted_by = count)]
items: wire::Array<T>,
```

Read controllers are authoritative. Write payload intent is authoritative: array length, byte
length, conditional presence, and computed values derive or patch their controllers while writing.
Controller setters are omitted.

The shipped dependency vertical accepts top-level byte-length and presence controllers with fixed
sequential geometry. Multiple payloads may share one byte-length controller, but their write
lengths must agree. A controller cannot simultaneously control placement. Nested controller paths,
cross-role dependencies, and non-scalar conditional bodies extend the same DAG in later verticals.

Controller paths may be nested but must identify physically earlier values for one-pass framing.
Generated builder patches are static type-level operations; paths do not exist at runtime.

Bitfields share one representation model in two source forms. A reusable nominal type declares its
physical integer on the item and its logical ranges on fields:

```rust
#[derive(WireView, WireBuilder)]
#[wire(as = u32, le)]
struct Flags {
    #[wire(bit = 0)]
    enabled: bool,
    #[wire(bits = 1..=3)]
    kind: u8,
}
```

For a few local bits, zero-width logical fields may project an earlier physical integer directly
with `bits_of = path` rather than requiring a wrapper type. Both forms compile to the same masks,
checked conversions, and generated state. A fresh builder zeros unassigned bits; exact View copying
preserves every source bit.

Padding, alignment, and forward-placement gaps are geometry, not implicit canonicality checks.
Views accept their source bytes, fresh builders write zeroes, and exact View copying preserves the
original representation. A protocol requiring a specific fill declares constant bytes explicitly.

## 6. Enums

Static enums declare one general selector representation and per-variant values:

```rust
#[derive(WireView, WireBuilder)]
#[wire(selector = u32, le)]
enum Value {
    #[wire(value = 1)]
    First(FirstBody),
    #[wire(value = 2)]
    Second(SecondBody),
}
```

Views expose a generated borrowed variant enum for ordinary exhaustive `match`. An explicit
`#[wire(unknown)]` variant preserves the raw selector and exact bounded or terminal body bytes, so
it can be forwarded through a writer without semantic reconstruction. Unknown selectors are
rejected with the raw selector and absolute offset when no unknown variant exists. A nonterminal
unknown body must receive a provable physical boundary; otherwise composition fails at its field
site.

Selector values are compile-time schema facts. Negotiated runtime selector maps are outside the
derive contract; a protocol that negotiates opcodes uses a manual wrapper capability.

## 7. Sequences and cursors

The product concepts are distinct:

- `views` traverses consecutive representations of one schema;
- `cursor` retains a position so different schemas can consume consecutive representations.

For a syntactically fixed struct or nominal bitfield, `views` prevalidates the complete input and
returns an infallible `ExactSizeIterator`. Closed enums and variable structs with a leading-extent
capability return a facade whose `next` is `Result<Option<View>, Error>`.
Direct `rest`, terminal arrays, and unknown enum bodies expose no helpers. A terminal child or
closed enum whose transitive body lacks a leading extent returns `SequenceError::Unavailable`
before consuming input.

Cursor usage is schema-led:

```rust
let (header, mut cursor) = Header::cursor(&input)?;
let body = Body::next(&mut cursor)?;
let remaining = cursor.remaining();
```

Views yielded by a cursor borrow the original backing, not the cursor, and may coexist. Failure or
`NeedMore` does not advance the cursor.

A runtime array getter returns a facade retaining only the collection range and authoritative
count. Its iterator frames one exact item per `next`; a repeated traversal replays item geometry.
The core does not retain or allocate a per-item range index.

## 8. Progressive writers

Generated writers use consuming typestate setters over caller-owned output. Missing required
fields and partially initialized conditional groups do not expose `finish`.

The base constructor is type-directed:

```rust
Packet::builder(&mut fixed_slice) // fixed, returns NeedMore
Packet::builder(&mut vec)         // growable through Extend<u8>
Packet::builder(&mut bytes_mut)   // the same capability bounds
```

wire-repr remains `no_std` and `no_alloc`: a growable output allocates only through the
caller-selected output type. `output::bounded` constrains a pooled collection to a size class;
`output::grow_with` delegates fallible or custom growth to a caller callback.

Nested derived schemas use closures:

```rust
Request::builder(output)
    .query(|query| query.field(value))?
    .finish()?
```

Derived and manual children use one closure setter. Public `WireBuilder` supplies detached child
typestate, and `WireWrite<V>` emits the closure result into the parent's progressive cursor. The
API does not generate field-suffixed `_value` or `_default` methods.

Conditional fields and groups use one collision-free choice closure:

```rust
packet.details(|details| match value {
    Some(value) => details.present(|details| details.value(value)),
    None => details.absent(),
})?
```

Generated code writes fixed fields directly at compile-time offsets, retains offsets rather than
pointers across relocations, and patches controllers or computed destinations when their
dependencies become available. Runtime collections use a streaming closure that writes one item at
a time and patches count or byte controllers without retaining item plans:

```rust
packet.items(|mut items| {
    for value in source {
        items = items.item(|item| item.value(value))?;
    }
    Ok(items)
})?
```

Generated views implement the same item write capability by copying their exact represented bytes.
A caller may therefore stream an array facade from one view into another writer or retain views in
caller-owned storage before writing. No semantic reconstruction, per-item plan, or hidden
allocation is required.

Errors are reported when discovered. Output may contain a partial unpublished representation;
wire-repr does not clear, restore, or roll back bytes. `finish()` returns `Written<O>` with the
exact represented range. Applications requiring atomic publication use staging, double buffering,
or buffer-pool ownership outside wire-repr.

## 9. Physical byte selections and computed fields

Generated struct views expose root-relative typed physical selections through the collision-free
`select(&view)` function:

```rust
select(&view).include(|fields| fields.header | fields.payload)
select(&view).exclude(|fields| fields.checksum)
```

Selections preserve physical order, merge overlap, and remain fragmented without materialization.
They expose `len()`, borrowed `chunks()`, byte iteration, and exact copying. A simple byte algorithm
may fold `selection.bytes()`; an optimized checksum feeds each `selection.chunks()` slice directly
to its update routine.

Computed callbacks retain the earlier field-expression syntax and may mix logical values with
physical selections:

```rust
#[wire(computed = crc32(exclude(self)))]
checksum: u32,

#[wire(computed = ordered_count(
    kind,
    include(first, tail),
    exclude(self, second),
))]
ordered: u32,
```

`self` names the computed destination independently of its Rust field name. Including `self` is a
cycle; excluding it is the ordinary checksum form. `try_computed` uses the same arguments but
expects a fallible callback marked with `#[computed]`. At `finish`, the generated dependency DAG
patches callbacks in topological order after payload controllers are authoritative. Computed
destinations cannot themselves control geometry, be conditional, declare placement, or follow a
demand-derived offset. Writing computed fields requires the schema's generated `WireView`
capability so the final patch pass can frame exact physical ranges without retaining per-field
plans. Reading returns the stored value and never recomputes it. A fallible callback may leave
partial unpublished output.

## 10. Errors and incomplete input

Generated read and build errors are nominal field-site enums derived with `thiserror`. Nested
errors retain their concrete source types. Read errors carry absolute root-input offsets.

Incomplete contiguous input returns:

```rust
NeedMore {
    offset,
    additional_at_least,
}
```

The amount is exact when provable and otherwise a lower bound. wire-repr does not own `Read`,
`AsyncRead`, segmented input, or resumable parser state. The caller appends to its buffer and
retries.

The core performs no hidden full-collection validation: dynamic iteration happens only through the
getter, iterator, or cursor operation requested by the caller. Checked arithmetic and input bounds
protect geometry; the application owns policy for how far it chooses to iterate. Layouts whose
physical boundaries are ambiguous are rejected by derive rather than guarded by an arbitrary
runtime budget.

## 11. Implementation order

The fixed, demand-geometry, dependency, collection, enum, bitfield, root-selection, computed,
sequence, cursor, fuzz, protocol-fixture, example, and release-verification verticals are complete.
Nested selection paths, traversal, and recursive schemas are separate future composition work.

Each shipped vertical owns its runtime, derive model, generated/idiomatic/best-safe workloads,
behavioral tests, fail-fast diagnostics, and documentation in one coherent commit. A phase does not
land as a compiling scaffold or with a second temporary renderer.

## 12. Performance and verification

Every shipped representation class must have generated, idiomatic, and best-safe implementations
with one semantic oracle. Optional unsafe implementations are informational lower bounds only.
Workload formulas own their hard gates and optimization-attention policy; outperforming idiomatic
code is a success, while a gap to best-safe remains visible without automatically failing CI.
The current mandatory corpus has fourteen discovered zones and thirty-four cases: fixed scalars and
constants, explicit logical conversions, one generic nested child, a four-level compound generic
lattice, fixed byte arrays with multiple nested children, dynamic geometry, controller and
conditional dependencies, runtime collection decode/build/copy, static enum decode/build/copy,
nominal and inline bitfields, fragmented physical selections, fixed and dynamic computed fields,
fixed and variable homogeneous views, heterogeneous cursors, and fixed, automatic, and
callback-driven output growth. Each covers read and write paths where applicable. The measurement
tool inspects final linked consumer symbols for code shape, call topology, stack, allocation, and
dispatch evidence.
Runtime performance uses calibrated interleaved samples and reports distribution statistics. LLVM
IR may explain an optimization result but is not treated as a latency oracle. State and
artifact-size probes remain isolated from measured hot-path implementations.
Nested selections become mandatory corpus coverage when that composition class ships.

Behavioral tests cover success, truncation at every accessed field boundary, constant mismatch,
declared outer-extent mismatch, nested error propagation, absolute offsets, retained backing
identity, typestate failures, manual capability composition, partial-output failure semantics,
growth adapters, and generated/handwritten equivalence. Fuzzing extends the same invariants to
controller overflow, count bombs, non-progress items, dependency cycles, malformed deferred
ranges, and iteration termination.

The release fixtures exercise generic MTProto TL composition and an IPv4 header with nominal
bitfields plus a computed Internet checksum. Deterministic structural fuzz cases cover controller
bombs, malformed dynamic ranges, collection termination, and failure-atomic sequence movement.

## 13. Explicit non-goals

The production core does not provide:

- runtime schema reflection or registries;
- negotiated runtime selector maps;
- hidden collection indexes or eager whole-tree validation;
- a general depth, item, or work-budget framework;
- dynamic dispatch or resolver frameworks;
- semantic/domain object materialization;
- mutable views;
- async or transport I/O;
- hidden allocation, `alloc` mode, or `bytes` mode;
- serde integration;
- canonicality inspection without a demonstrated consumer;
- compatibility aliases for the legacy `Wire` API.

The source API may be broad and convenient. Unused convenience must disappear after
monomorphization, and used paths must remain comparable to ordinary handwritten safe Rust.
