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

## Output ownership

Fixed slices and borrowed growable collections remain the shortest writer inputs. The generic
`output::owned(target)` adapter instead stores any contiguous
`AsRef<[u8]> + AsMut<[u8]> + Extend<u8>` target by value. It adds no allocator dependency and
allocates only through the target's `Extend` implementation. Unfinished writer states inherit the
target's `Send` and `'static` properties, so an owned `Vec`, `BytesMut`, or compatible collection
may cross an ordinary worker channel. `Written::into_parts()` returns the adapter and exact range;
`Owned::into_inner()` recovers the target.

## Recursive array/object views and writers

A closed selector enum may pass itself directly to a generic body containing either an unsigned
stored count followed by terminal `wire::Array<T>` or object fields separated by
`wire::Recursive<T>`. Recursive demand bodies may retain a leading unsigned byte controller across
a child to bound following raw bytes, then apply padding/alignment before a fixed scalar and the
next child. Such roots expose `Schema::view::<DEPTH>(backing)` for a caller-selected const depth;
zero returns `DepthExceeded`.

Framing uses one iterative typed continuation stack. Payloads are selected at compile time and use
transparent byte-packed representations: plain `u32` counts use four bytes, a plain `u64`
controller plus resume tag uses nine, and plain `u128` uses seventeen. Child-relative alignment
adds one packed `usize` body origin only to that grammar. Multiple root body grammars form one
generated continuation enum whose size is the maximum active payload plus its discriminant, not
the sum of per-grammar stacks.

Recursive item getters return the same generated root view family. Arrays retain at most 384 bytes
for exact fixed, affine-formula, interval-event, ranked-palette, factorized, recursive-shape,
periodic-palette, or packed-run geometry. Every candidate is validated across the complete
represented sequence before selection; failures use exact prefix replay. No mode stores item
offsets, and a forward iterator always retains one physical cursor.
Schema-specific constant, conversion, validator, and manual-leaf errors crossing recursive
repetition retain their absolute offset but flatten to finite `RecursiveError::Child` values.

Recursive object bodies compile physical segments and child markers into static `start` and
`resume` transitions. Their views retain direct field boundaries only; a recursive child getter
re-frames its already-proven exact range into the same generated root view family.

Deriving `WireBuilder` generates an output-owning recursive root writer. Object fields transfer the
cursor through physical-order typestate stages; recursive array items stream through the same
root-wide callback and patch their count after completion. The callback is monomorphized and
retains no recursive tree, plan, allocation, or hidden depth stack. Exact recursive views can be
copied through either the progressive root writer or the root's copy-only detached capability.
Generated aliases such as `ValueRootWriter<C>`, `ValuePairWriter<C>`, and
`ValueWriteResult<T, C>` allow named helpers to move current writer states without naming hidden
callback or slot types.

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
- Typed selections expose merged root-relative chunks and zero-sized paths through nested children.
- Computed scalar destinations patch from logical fields and physical selections in DAG order.
- Syntactically fixed structs/bitfields expose prevalidated `ExactSizeIterator` views; closed enums
  and variable structs frame lazily.
- Heterogeneous cursors yield coexisting views and never advance on failure.
- Recursive enum arrays and object bodies retain no per-item offset index, accept caller-selected
  depths beyond 64, and fail with `DepthExceeded` before crossing that bound.
- Progressive recursive writers stream object and array children directly, retaining only the
  output cursor, current count/controller offset, and compile-time typestate.
- Fixed writers return `NeedMore`; growable collections use their existing `Extend<u8>` capability.
- Write failure may leave partial unpublished bytes. `finish()` returns the exact represented range.
- Generated and manual writers allocate nothing inside wire-repr and dispatch statically.

General traversal is the remaining future composition surface. The core does not add negotiated
selector maps, hidden indexes, general resource-limit machinery, runtime schemas, semantic object
materialization, async I/O, or feature-selected renderers.
