<h1 align="center">wire-repr</h1>

<p align="center"><strong>Compile Rust wire schemas into zero-copy views and progressive writers.</strong></p>

<p align="center"><code>no_std</code> · no allocation · safe public API · Rust 1.91</p>

`wire-repr` treats a Rust schema declaration as a physical representation, not as the decoded
value. Reading returns an opaque exact-source view over the caller's backing. Writing uses a
typestate writer over caller-owned fixed, growable, bounded, or custom output.

The implemented production surface is the featureless `WireView`/`WireBuilder` capability model.
[`ARCHITECTURE.md`](ARCHITECTURE.md) defines the complete layout contract.

## Add it

```toml
[dependencies]
wire-repr = "1"
```

The crate is featureless. Applications may pass `bytes::Bytes`, `Vec<u8>`, slices, or custom
`AsRef<[u8]>` backing without enabling a wire-repr feature.

## Generic exact-source view and progressive writer

```rust
use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct HelpGetConfig {
    #[wire(le, constant = 0xc4f9_186b)]
    constructor: u32,
}

#[derive(WireView, WireBuilder)]
struct InvokeWithLayer<T> {
    #[wire(le, constant = 0xda9b_0d0d)]
    constructor: u32,
    #[wire(le)]
    layer: i32,
    query: T,
}

type Query = InvokeWithLayer<HelpGetConfig>;

let input = [
    0x0d, 0x0d, 0x9b, 0xda, // invokeWithLayer
    0xc8, 0x00, 0x00, 0x00, // layer 200
    0x6b, 0x18, 0xf9, 0xc4, // help.getConfig
];
let view = Query::view(input).unwrap();
assert_eq!(view.layer(), 200);
assert_eq!(view.query().constructor(), 0xc4f9_186b);
assert_eq!(view.as_bytes(), &input);

let mut output = [0xa5; 16];
let written = Query::builder(&mut output[..])
    .layer(200)?
    .query(|query| query)?
    .finish()?;
assert_eq!(written.range(), 0..12);
assert_eq!(written.as_bytes(), &input);
assert_eq!(&output[12..], &[0xa5; 4]);
```

`view(input)` stores `input` directly. Passing a reference creates a borrowed view; passing an
owned container moves it into the hidden view state. Nested getters borrow their parent and use
retained reference-free geometry state rather than reparsing.

Retained backing must keep projecting the same immutable byte span while the view exists. Slices,
`Vec`, `bytes::Bytes`, and ordinary owned wrappers satisfy this. Stateful `AsRef` implementations
that switch or mutate their projection are not supported.

Constants are validated on read and have getters, but no writer setters. Derived and manual
children use the same closure setter through public `WireBuilder` and `WireWrite<V>` capabilities.
Setters write progressively; only offsets and typestate remain in the generated writer.

## Shared controllers and conditional groups

A byte-length controller may govern multiple payloads. Reads trust the stored controller; writes
derive it from payloads and reject conflicting lengths. Presence uses a physical bool controller,
a zero-width logical flag, and contiguous dependent fields:

```rust
#[derive(WireView, WireBuilder)]
struct Foo {
    #[wire(as = u8)]
    present: bool,
    #[wire(flag = present)]
    details: bool,
    #[wire(depends_on = details)]
    value: u8,
}

Foo::builder(output)
    .details(|choice| match value {
        Some(value) => choice.present(|details| details.value(value)),
        None => choice.absent(),
    })?
    .finish()?;
```

The physical controller has no setter. The choice closure returns one unified type for both
branches, while the present branch uses typestate to require every dependent field.

## Runtime collections

Controllers remain physical fields while `wire::Array<T>` marks the repeated representation:

```rust
count: u16,
#[wire(counted_by = count)]
items: wire::Array<T>,
```

The getter retains only the array range and authoritative count. Each `iter()` call replays item
geometry without allocating an index. Writers stream items through one closure and patch count
afterward. `item_view` and `item_result` copy individual exact views; `copy_from(source.items())`
forwards one validated array range without semantic reconstruction or per-item writes.

## Static enums and bitfields

Static enums use one physical selector and expose a borrowed exhaustive variant enum. An
`#[wire(unknown)]` variant retains the raw selector and exact bounded or terminal body, so
`item_view` can forward it unchanged.

Nominal bitfields declare `#[wire(as = u32, le)]` on the type and `bit`/`bits` on logical fields.
For local projections, `#[wire(bits_of = raw, ...)]` derives zero-width getters and builder setters
from an earlier unsigned scalar. Fresh builders zero undeclared bits; exact views preserve them.

## Physical selections and computed fields

Use the collision-free `select(&view)` entry point; it reserves no generated schema method name:

```rust
use wire_repr::select;

let selected = select(&view).exclude(|fields| fields.checksum);
let nested = select(&view).include(|fields| {
    fields.header.fields(|header| header.kind | header.flags)
        | fields.payload
});
```

Selections retain no flattened buffer. `chunks()` merges adjacent or overlapping spans in physical
order, while `bytes()` iterates the same fragmented representation. Nested paths are zero-sized
types with no runtime depth limit, path buffer, allocation, or dispatch. A manual child remains
selectable whole; descending through it requires an explicit unsafe field-schema implementation
that binds its typed field family to exact range hooks. Stored scalar destinations use
`computed = callback(...)`; fallible callbacks use `try_computed` plus `#[computed]` error metadata.
Callbacks may mix logical getters with `include(...)` and
`exclude(...)`, including paths such as `include(header.kind, payload)`. The generated dependency
DAG orders computed patches independently of declaration order. Destinations must precede
demand-derived offsets, while their selections may consume demand-framed fields.

Schemas that write computed fields derive both `WireView` and `WireBuilder`; the generated read
capability supplies the exact structural ranges used during the final patch pass.

## Consecutive views and cursors

Syntactically fixed structs and nominal bitfields prevalidate the complete input and return an
infallible `ExactSizeIterator`. Closed enums and variable structs with a leading extent return a
lazy facade whose `next()` reports the first bad item. Heterogeneous cursors advance only after
successful framing:

```rust
for item in Foo::views(&fixed_input)? {
    consume(item);
}

let (header, mut cursor) = Header::cursor(&input)?;
let body = Body::next(&mut cursor)?;
let remaining = cursor.remaining();
```

Yielded views borrow the original input rather than the facade or cursor, so they may coexist.

Direct terminal `rest`, terminal arrays, and unknown enum bodies expose no sequence or cursor
helpers. A terminal child or closed enum whose transitive body lacks a leading extent returns
`SequenceError::Unavailable` before consuming input.

## Manual wire types


Manual representations implement the same independent read and write capabilities as derived
schemas.

```rust
use wire_repr::{ChildWriter, Output, WireBuilder, WireWrite, WriteError};

struct LittleEndianWord;

impl WireBuilder for LittleEndianWord {
    const FIXED_SIZE: Option<usize> = Some(4);

    type Builder = ();

    fn builder() -> Self::Builder {}
}

impl WireWrite<u32> for LittleEndianWord {
    type Error = core::convert::Infallible;

    fn write<O: Output>(
        value: u32,
        writer: &mut ChildWriter<'_, O>,
    ) -> Result<(), WriteError<Self::Error, O::GrowError>> {
        writer.write(&value.to_le_bytes())?;
        Ok(())
    }
}
```

Manual writers receive the same progressive cursor as generated children. `FIXED_SIZE` enables a
manual child before later physical fields. A variable-width manual child is terminal unless the
parent bounds it with `#[wire(bytes = earlier_length)]`. Manual writers may return semantic errors
after partially modifying unpublished output; wire-repr never allocates, rolls back, or clears
bytes.

Manual `WireView` implementations are an explicit unsafe boundary: retained state must remain
memory-safe for any immutable span of the framed length. Generated APIs remain safe, retain
validated logical values when needed, and check manual child extents before reconstruction.

## Scalar representations

The schema model handles `u8`/`i8` and every 16-, 32-, 64-, and 128-bit integer in both byte
orders, plus `f32` and `f64`. One-byte fields have no endian attribute; every multibyte field
requires `le` or `be`.

Platform and logical Rust types declare their physical width explicitly:

```rust
#[derive(WireView, WireBuilder)]
struct Index {
    #[wire(as = u32, le)]
    offset: usize,
    #[wire(as = i64, be)]
    delta: isize,
    #[wire(as = u8)]
    enabled: bool,
    #[wire(as = u32, le)]
    character: char,
}
```

Read and write conversions are checked. Invalid stored values and values that do not fit their
declared wire width produce nominal field-site errors rather than truncating.

## Behavioral guarantees

- `view()` accepts exactly one representation and rejects trailing input.
- Errors identify the field site and absolute root-input offset.
- `NeedMore` reports a proven lower bound for incomplete contiguous input.
- Derived descriptors contain no input references or self-references; manual descriptors certify
  the same invariant through the unsafe `WireView` contract.
- Ordinary scalar getters remain ordinary scalar values.
- Generic and nested composition is static and monomorphized.
- `builder(&mut [u8])` writes into fixed output and returns `OutputError::NeedMore` when it ends.
- `builder(&mut Vec<u8>)` and `builder(&mut bytes::BytesMut)` grow automatically through
  `AsRef<[u8]> + AsMut<[u8]> + Extend<u8>`.
- `output::bounded` and `output::grow_with` opt into bounded or caller-controlled growth.
- On write failure, output may contain a partial unpublished representation. `finish()` returns a
  `Written` token with the exact represented range.
- Writers and views do not allocate or dispatch dynamically inside wire-repr.
- The public read API is identical for borrowed and retained-owned backing.

## Recursive arrays, object continuations, and writers

A closed selector enum may recurse through a generic counted-array body or a fixed sequential
object body containing `wire::Recursive<T>`. The caller selects a compile-time depth; zero returns
`DepthExceeded`, while positive values reserve one `[MaybeUninit<u32>; DEPTH]` continuation stack,
or `4 * DEPTH` bytes. Multiple recursive body grammars add a generated u8/u16 kind stack. Generated
code never uses recursive Rust calls, allocation, or a per-item offset index. Recursive stored
counts must fit `u32` even when their physical controller is wider.

```rust
#[derive(WireView, WireBuilder)]
struct Values<T> {
    #[wire(le)]
    count: u16,
    #[wire(counted_by = count)]
    items: wire_repr::wire::Array<T>,
}

#[derive(WireView, WireBuilder)]
struct Pair<T> {
    left: wire_repr::wire::Recursive<T>,
    opcode: u8,
    right: wire_repr::wire::Recursive<T>,
}

#[derive(WireView, WireBuilder)]
struct Leaf {
    value: u8,
}

#[derive(WireView, WireBuilder)]
#[wire(selector = u8)]
enum Value {
    #[wire(value = 0)]
    Leaf(Leaf),
    #[wire(value = 1)]
    Array(Values<Value>),
    #[wire(value = 2)]
    Pair(Pair<Value>),
}

let root = Value::view::<128>(input)?;
let ValueVariant::Array(array) = root.variant() else {
    panic!("array")
};
let fifth = array.items().get(5)?;
```

Recursive arrays retain at most 384 bytes of compact geometry. Exact fixed, affine-formula,
interval-event, ranked-palette, factorized, recursive-shape, periodic-palette, and packed-run
descriptors are selected only after generated framing validates every represented item. Any failed
candidate falls back to exact prefix replay; no mode stores item offsets. `get(n)` therefore has
mode-dependent complexity. `iter()` always keeps one forward cursor and remains linear in the
complete represented range.

`wire::Recursive<T>` is the zero-sized schema marker required by Rust to break the nominal type
cycle in an object body. Fixed sequential scalars and byte arrays may appear before, between, or
after recursive fields. Generated `start`/`resume` transitions skip the body iteratively; direct
child getters re-frame only their already-proven exact ranges. Schema-specific errors crossing a
recursive boundary retain their absolute offset but flatten to a finite `RecursiveError::Child`.

Deriving `WireBuilder` on the body and root generates a progressive recursive writer:

```rust
Value::builder(output)
    .pair(|pair| {
        let pair = pair.left(|value| value.leaf(|leaf| leaf.value(10)))?;
        let pair = pair.opcode(7)?;
        pair.right(|value| value.leaf(|leaf| leaf.value(20)))
    })?
    .finish()?
```

The cursor moves by value through generated typestate stages, so no recursive value tree, plan,
allocation, or hidden depth stack is retained. Object fields are written in physical order.
Recursive arrays stream items and patch their count after the closure; exact root views may be
copied directly. The detached `WireBuilder` path for a recursive root deliberately exposes exact
View copying only—semantic recursive construction remains progressive.

## Examples

- `cargo run -p wire-repr --example mtproto` demonstrates generic nested TL constructors.
- `cargo run -p wire-repr --example ipv4` demonstrates nominal bitfields and a computed Internet
  checksum over physical selections.

## Verification

The repository checks behavior, final linked artifacts, and runtime performance independently:

```text
cargo +1.91.0 test --workspace --all-targets
cargo +1.91.0 clippy --workspace --all-targets -- -D warnings
python3 ci/check-fail-fast.py
cargo +1.91.0 run -p wire-repr-measure --release -- run
```

The Rust measurement tool discovers capability-owned workloads below `wire-repr/measure`. Each
workload supplies generated, idiomatic, and best-safe implementations plus optional lower bounds.
Its own formulas decide hard failures, optimization attention, and additional derived metrics.
Human-readable output is the default; CI uses `run --json`. Artifact analysis reads final linked
symbols, while interleaved calibrated samples report median, p95, range, and median absolute
deviation instead of treating LLVM instruction counts as performance truth.

Release verification also runs deterministic structural fuzz regressions, MTProto and IPv4
fixtures, both public examples, cross-target checks, rustdoc, and package-content validation.

## Design direction

The implemented surface currently covers fixed scalars and byte arrays, constants, explicit
logical conversions, validators, nested children, demand geometry, controller dependencies,
conditional groups, runtime arrays, static enums with exact unknown forwarding, nominal and inline
bitfields, nested physical selections, computed fields, homogeneous `views`, heterogeneous
cursors, exact View forwarding, depth-bounded recursive enum arrays, recursive object
continuations, progressive recursive object/array writers, and ordinary progressive typestate
writers.

General traversal is the remaining future composition surface. Negotiated selector maps, hidden
indexes, eager semantic-tree validation, and a general limits framework are not part of the target
core. Every shipped class adds behavioral plus generated/idiomatic/best-safe workload evidence.
