# Architecture

This document defines the target production architecture and clean-cutover contract for
`wire-repr`. The current implementation is the fixed/generic vertical: named structs, fixed
scalars, constants, explicit logical conversions, schema validators, and one terminal
nested child with generic and manual capability composition. Later sections describing enums,
dynamic geometry, collections, computed fields, selections, cursors, and limits are an
implementation plan, not claims about the shipped surface. New layout classes extend this one
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
its lifetime. Nested getters return small views borrowing their parent. They do not clone or slice
owned backing into independent handles.

`view()` requires one exact representation. `view_unchecked()` skips declared semantic validators,
not structural safety.

## 3. Framing and descriptors

Framing establishes only the geometry required to locate and safely expose fields. Values remain
lazy. Fixed scalar getters decode from exact source bytes when called.

Every schema has a static read capability with:

- a lifetime-free typed error;
- a reference-free `State: 'static` descriptor;
- one leading-frame operation;
- a borrowed nested-view family;
- fixed-width metadata when provable.

Top-level view state owns the descriptor. A nested child view borrows its exact input span and the
corresponding descriptor state. Descriptors never contain input references, self-references,
runtime schema objects, or dynamic dispatch.

Generated descriptors are specialized per schema:

- fixed offsets occupy no geometry state;
- terminal child state is stored directly;
- sequential dynamic geometry stores only necessary endpoints;
- controller values are retained only when later geometry needs them;
- recursive and repeated layouts do not allocate per-item indexes by default.

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

Controller paths may be nested but must identify physically earlier values for one-pass framing.
Generated builder patches are static type-level operations; paths do not exist at runtime.

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

Views expose a generated borrowed variant enum for ordinary exhaustive `match`. If an explicit
`#[wire(unknown)]` variant exists, unknown selectors are preserved; otherwise they are rejected
with the raw selector and absolute offset.

A non-generic enum may opt into negotiated runtime selector values with `dynamic_values`.
The derive generates a typestate values builder with one setter per variant and duplicate-value
validation. A configured outer schema binds values through `Schema::with(&values)`. Dependencies
propagate statically through concrete nested schemas. Values are used only during framing or
progressive writing and are not retained in views after those operations.

Small dynamic enums use inlined comparisons; larger enums use a generated sorted lookup. Both
strategies require handwritten codegen and latency comparators.

## 7. Sequences and cursors

The product concepts are distinct:

- `views` traverses consecutive representations of one schema;
- `cursor` retains a position so different schemas can consume consecutive representations.

For a fixed schema, `views` performs one upfront sequence check and returns an infallible
`ExactSizeIterator`. For a variable schema, `views` returns a facade whose `next` is
`Result<Option<View>, Error>`. The first invalid item is fail-closed; later boundaries are not
trusted or skipped.

Cursor usage is schema-led:

```rust
let (header, mut cursor) = Header::cursor(&input)?;
let body = Body::next(&mut cursor)?;
let remaining = cursor.remaining();
```

Views yielded by a cursor borrow the original backing, not the cursor, and may coexist. Failure,
`NeedMore`, or resource exhaustion does not advance the cursor.

A runtime array getter returns the element schema's `views` facade directly. Variable collections
may replay item boundary traversal when accessed after parent framing. A future indexed mode may
accept caller-owned storage, but the core never allocates an index silently.

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

Generated code writes fixed fields directly at compile-time offsets, retains offsets rather than
pointers across relocations, and patches controllers or computed destinations when their
dependencies become available. Runtime collections write items as caller code supplies them; no
one-shot iterator or per-item plan is buffered silently.

Errors are reported when discovered. Output may contain a partial unpublished representation;
wire-repr does not clear, restore, or roll back bytes. `finish()` returns `Written<O>` with the
exact represented range. Applications requiring atomic publication use staging, double buffering,
or buffer-pool ownership outside wire-repr.

## 9. Physical byte sources and computed fields

Views and writers expose root-relative typed physical selections:

```rust
view.bytes().include(|fields| fields.header | fields.body.payload)
view.bytes().exclude(|fields| fields.checksum)
```

Selections preserve physical order and remain fragmented. They expose borrowed chunks, virtual
fills, bounded ranges, and byte iteration without packet-sized materialization.

For `WireView`, a computed field is the stored decoded value; reading does not recompute it.
For `WireBuilder`, generated dependency order determines when a computed callback can run.
Callbacks consume selections over already written output and minimal retained values. Fallible
computed callbacks may leave partial unpublished output. Self-dependency, cycles, missing fields,
and geometry cycles remain derive errors.

## 10. Errors, limits, and incomplete input

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

Resource policy precedence is:

```text
library default < schema default < call-site override
```

Configured schemas preserve the complete entrypoint surface:

```rust
Packet::limits(Limits::<64>::new().work(100_000)).view(input)?
```

Depth capacity is compile-time so no variable-length stack is allocated. Work budget is runtime.
Simple schemas pay no limits cost after optimization. `LimitExceeded` is fail-closed.

Recursive grammars compile to iterative state machines. Homogeneous recursive arrays use counters;
continuation stacks are generated only when the grammar requires them. Recursive Rust calls are
not the default implementation.

## 11. Performance and verification

Every shipped representation class must have generated, idiomatic, and best-safe implementations
with one semantic oracle. Optional unsafe implementations are informational lower bounds only.
Workload formulas own their hard gates and optimization-attention policy; outperforming idiomatic
code is a success, while a gap to best-safe remains visible without automatically failing CI.

The current mandatory corpus has four discovered zones and ten cases: fixed scalars and constants,
explicit logical conversions, one generic nested child, and a four-level compound generic lattice,
each covering read and write paths where applicable. The measurement tool inspects final linked
consumer symbols for code shape, call topology, stack, allocation, and dispatch evidence. Runtime
performance uses calibrated interleaved samples and reports distribution statistics. LLVM IR may
explain an optimization result but is not treated as a latency oracle. State and artifact-size
probes remain isolated from measured hot-path implementations.

Bounded children, conditional groups, arrays, recursive enums, heterogeneous builders, runtime
collections, selections, and computed fields become mandatory corpus zones when those layout
classes ship; they are not claimed as current measurement coverage.

Behavioral tests cover success, truncation at every field boundary, constant mismatch, trailing
input, nested error propagation, absolute offsets, retained backing identity, typestate failures,
manual capability composition, partial-output failure semantics, growth adapters, and generated/
handwritten equivalence. Fuzzing extends the same invariants to controller overflow, count bombs,
non-progress items, dependency cycles, and depth/work exhaustion.

## 12. Explicit non-goals

The production core does not provide:

- runtime schema reflection or registries;
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
