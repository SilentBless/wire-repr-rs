# Wire schemas, exact-source views, and progressive writers

`wire-repr` compiles Rust schema declarations into a safe public, `no_std`, allocation-free read
and write API. A schema struct describes physical bytes; it is not the decoded semantic value.

The production `WireView`/`WireBuilder` contract is implemented across fixed and dynamic layouts,
dependencies, collections, enums, bitfields, selections, computed fields, sequences, and cursors.
[`ARCHITECTURE.md`](https://github.com/SilentBless/wire-repr-rs/blob/main/ARCHITECTURE.md) defines
the shipped boundaries and deferred composition work.

## Generic schema

```rust
use wire_repr::{WireBuilder, WireView};
# fn main() -> Result<(), Box<dyn std::error::Error>> {

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
    0x0d, 0x0d, 0x9b, 0xda,
    0xc8, 0x00, 0x00, 0x00,
    0x6b, 0x18, 0xf9, 0xc4,
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
# Ok(())
# }
```

`view<T: AsRef<[u8]>>(input: T)` stores `T` directly. Passing a reference borrows; passing an
owned container moves it into the opaque view. No wire-repr feature selects another API.
Nested views borrow their parent and use retained reference-free descriptor state.

Retained backing must expose the same immutable byte span for the view's lifetime. Slices, `Vec`,
`bytes::Bytes`, and ordinary owned wrappers satisfy this; stateful `AsRef` projections do not.

Constants are validated on read, exposed by getters, and omitted from writers. Generic children
write through their static capabilities and the same progressive output cursor.

## Manual capabilities

Manual representations implement read and write independently. Manual read implementations are an
explicit audited unsafe boundary because they certify retained state for later zero-cost
reconstruction. Manual writers use the safe public progressive cursor.

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

Generated parents configure both derived and manual children through the same closure setter.
The child exposes its detached initial state through `WireBuilder`; `WireWrite<V>` writes the
closure result directly into the parent's output. A manual `FIXED_SIZE` allows later physical
fields while bounded child cursors prevent over- or under-writing that declared region.

Primitive schemas support all fixed-width Rust integers and floats. `usize`, `isize`, `bool`, and
`char` require an explicit physical representation such as `#[wire(as = u32, le)]`; both read and
write conversions are checked.

## Recursive array views

A closed selector enum may pass itself directly to a generic body containing an unsigned stored
count followed by a terminal `wire::Array<T>`. Such roots expose `Schema::view::<DEPTH>(backing)`
for a caller-selected const depth; zero returns `DepthExceeded`. Framing uses an iterative
`[MaybeUninit<u32>; DEPTH]` continuation stack, so stack cost is `4 * DEPTH` bytes rather than
hidden allocation or recursive Rust calls. Recursive count values must fit `u32`.

Recursive item getters return the same generated root view family. Arrays retain at most 384 bytes
for exact fixed, affine-formula, interval-event, ranked-palette, factorized, recursive-shape,
periodic-palette, or packed-run geometry. Every candidate is validated across the complete
represented sequence before selection; failures use exact prefix replay. No mode stores item
offsets, and a forward iterator always retains one physical cursor.
Schema-specific constant, conversion, validator, and manual-leaf errors crossing recursive
repetition retain their absolute offset but flatten to finite `RecursiveError::Child` values.

## Guarantees

- `view()` accepts one exact representation and rejects trailing input.
- Generated errors retain field sites, concrete nested sources, and absolute offsets.
- Derived descriptors are reference-free and `State: 'static`; manual implementations certify the
  same invariant through unsafe `WireView`.
- Scalar getters decode lazily from exact source bytes.
- Fixed `[u8; N]` fields and constants preserve their exact bytes without endian conversion.
- Multiple fixed nested children retain independent state and field-site errors.
- `wire::Bytes` exposes bounded or terminal source spans without copying.
- Padding and placement gaps remain opaque on read and are zero-filled by fresh writers.
- Bounded raw and nested payloads patch their physically earlier length controllers.
- Shared byte-length controllers reject conflicting write intent without hidden plans.
- `flag` and `depends_on` generate one present/absent choice closure for coherent groups.
- `wire::Array<T>` exposes replayable range-and-count facades without retaining item indexes.
- Streaming array writers patch count and accept exact generated or traversed item views.
- Exact array forwarding copies one validated collection range and patches its authoritative count.
- Static enums expose borrowed exhaustive variants and preserve exact unknown bodies.
- Nominal and inline bitfields use checked logical ranges while fresh writers zero undeclared bits.
- Root-relative typed selections expose merged chunks and physical-order byte iteration.
- Computed scalar destinations patch from logical fields and physical selections in DAG order.
- Syntactically fixed structs/bitfields expose prevalidated `ExactSizeIterator` views; closed enums
  and variable structs frame lazily.
- Heterogeneous cursors yield coexisting views and never advance on failure.
- Recursive enum arrays retain no per-item offset index, accept caller-selected depths beyond 64,
  and fail with `DepthExceeded` before crossing that bound.
- Fixed writers return `NeedMore`; growable collections use their existing `Extend<u8>` capability.
- Write failure may leave partial unpublished bytes. `finish()` returns the exact represented range.
- Generated and manual writers allocate nothing inside wire-repr and dispatch statically.

Nested selection paths, general traversal, recursive object continuations, and recursive builders
remain future composition work. The core does not add negotiated selector maps, hidden indexes,
general resource-limit machinery, runtime schemas, semantic object materialization, async I/O, or
feature-selected renderers.
