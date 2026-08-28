# Exact-source wire views and progressive writers

`wire-repr` compiles Rust schema declarations into immutable views over exact source bytes and
output-owning typestate writers. The generated target API is `no_std`, allocation-free, and
statically dispatched.

A schema describes physical representation rather than a decoded semantic object. The library owns
byte order, widths, field order, framing, selectors, controllers, geometry, and exact represented
ranges. Protocol and application semantics remain in consumer code.

## Quick start

```
use wire_repr::{WireBuilder, WireView, wire};

#[derive(WireView, WireBuilder)]
struct Packet {
    #[wire(be, constant = 0x5752)]
    magic: u16,
    kind: u8,
    #[wire(be)]
    payload_len: u16,
    #[wire(bytes = payload_len)]
    payload: wire::Bytes,
}

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let input = [0x57, 0x52, 7, 0, 5, b'h', b'e', b'l', b'l', b'o'];
let packet = Packet::view(&input)?;

assert_eq!(packet.magic(), 0x5752);
assert_eq!(packet.kind(), 7);
assert_eq!(packet.payload(), b"hello");
assert_eq!(packet.as_bytes(), input);

let mut output = Vec::new();
let written = Packet::builder(&mut output)
    .kind(7)?
    .payload(&b"hello"[..])?
    .finish()?;

assert_eq!(written.as_bytes(), input);
# Ok(())
# }
```

The stored length remains visible on the view, but the writer derives it from `payload`; no length
setter is generated. Constants are validated and exposed on views, then emitted automatically.

See the runnable [packet example](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.0/wire-repr/examples/packet.rs).

## Retained views

A generated `Schema::view<T: AsRef<[u8]>>(input: T)` stores `T` directly. Passing a reference
creates a borrowed root view. Passing an array, `Vec`, `bytes::Bytes`, or another owned wrapper moves
that backing into the opaque root view. There is no separate owned renderer or schema lifetime.

Framing proves the exact root geometry and rejects trailing bytes. Fixed scalar getters decode from
the original bytes when called. Nested views borrow their parent and reconstruct only exact ranges
whose state was already proven. Dynamic collection items may be framed lazily as their iterator or
getter advances.

Retained backing must keep projecting the same immutable byte span while the view exists. Ordinary
slices and immutable contiguous collections meet this contract. Intentionally stateful `AsRef`
implementations that switch their projection do not.

## Progressive writers

A generated `Schema::builder(output)` immediately owns or borrows one progressive output cursor.
Each field setter consumes one typestate stage and returns the next, so `?` is the ordinary control
flow. `finish()` is available only after all required fields have been supplied and returns a
[`Written`] publication token with the exact represented range.

Output behavior is type-directed:

- `&mut [u8]` is fixed and returns [`OutputError::NeedMore`](crate::OutputError::NeedMore);
- `&mut Vec<u8>` and `&mut bytes::BytesMut` grow through `Extend<u8>`;
- [`output::bounded`] constrains a target to a caller-selected limit;
- [`output::grow_with`] delegates fallible or custom growth to a callback;
- [`output::owned`] stores a growable target by value, allowing unfinished writer states to inherit
  its `Send` and `'static` properties.

Generated writers retain offsets rather than pointers and reacquire the mutable slice after growth,
so relocation is safe. The crate does not allocate by itself; a growable target allocates only
through its own implementation.

Writing is progressive, not transactional. An error may leave partial unpublished bytes. A caller
that requires atomic publication should write into an unpublished slot, staging buffer, or double
buffer and publish only after `finish()` succeeds.

## Scalars and logical representations

Direct scalar fields support fixed-width Rust integers and floats. Multibyte scalars require `be`
or `le`; one-byte integers do not accept an endian marker. Primitive fixed arrays use the same
per-element representation:

```
use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Lanes {
    #[wire(le)]
    lanes: [u16; 8],
}
```

Platform-sized and logical types declare a fixed physical representation:

```
use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Index {
    #[wire(as = u32, le)]
    offset: usize,
    #[wire(as = u16, be)]
    count: core::num::NonZeroU16,
}
```
User-defined newtypes use the same form when they implement the required conversions.


Reading requires `Logical: TryFrom<Physical>`. Writing requires `Physical: TryFrom<Logical>`.
Conversion failures remain nominal field-site [`ScalarConversionError`] or
[`ScalarBuildConversionError`] values; implementation-specific conversion error types do not leak
through generated public error enums. Stored constants use the direct physical representation.

## Geometry and dependencies

Declaration order is physical order. Generated framing and writing support:

- bounded raw bytes and nested children controlled by physically earlier unsigned scalars;
- terminal `rest` bytes;
- padding, alignment, and forward placement;
- shared byte-length controllers whose write payloads must agree;
- zero-width logical flags and contiguous conditional choice groups;
- runtime arrays controlled by physically earlier counts.

Read controllers are authoritative. Write payload intent is authoritative: generated writers patch
lengths, counts, presence bits, and bit projections when their dependent values become known.
Controller setters are omitted.

Padding, alignment, and placement gaps are geometry rather than canonicality checks. Views accept
their exact source bytes. Fresh writers fill forward gaps with zeroes. Exact view copying preserves
the source representation.

## Runtime arrays

[`wire::Array<T>`](crate::wire::Array) marks a repeated representation while the stored count remains
an ordinary physical field. [`ArrayView`] retains only the exact collection range and authoritative
count. It implements `IntoIterator` by value and by reference; iteration keeps one forward cursor
and does not retain item offsets.

[`ArrayWriter::try_extend`] consumes any `IntoIterator` and writes one item at a time. Exact item
views can be copied without semantic reconstruction. [`ArrayWriter::copy_from`] forwards an already
validated source array as one represented range and patches its authoritative count.

## Static enums and bitfields

A static enum declares one physical selector with `#[wire(selector = Physical, endian)]`. Unit
variants write only the selector. Body variants expose borrowed views and closure-based writer
methods. An explicit `#[wire(unknown)]` body preserves the raw selector and exact bounded or
terminal body for lossless forwarding.

Nominal bitfields declare their physical unsigned integer on the type and `bit` or `bits` ranges on
logical fields. Inline `bits_of` projections expose logical fields from an earlier physical scalar.
Fresh writers zero undeclared bits; exact view copying preserves every source bit.

## Physical selections and computed fields

[`select`] resolves typed physical field expressions against a generated view. A [`Selection`]
exposes merged wire-order [`Selection::chunks`] and byte iteration without materializing a flat
buffer. Nested field routes are zero-sized types and impose no runtime depth limit or path storage.

Computed scalar destinations use `computed = callback(...)`; fallible callbacks use
`try_computed`. Callback arguments may combine logical getters with `include(...)` and
`exclude(...)` physical selections. The generated dependency DAG evaluates and patches computed
fields in topological order.

The complete [IPv4 example](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.0/wire-repr/examples/ipv4.rs)
uses nominal bitfields and computes the Internet checksum over all physical header bytes except the
checksum destination.

## Sequences and cursors

Syntactically fixed schemas expose prevalidated [`FixedViews`], an infallible
`ExactSizeIterator`. Variable schemas with a provable leading extent expose [`VariableViews`], whose
`next` frames one representation lazily. [`Cursor`] consumes heterogeneous schemas from one shared
input.

Views yielded by these facades borrow the original input rather than the facade or cursor, so they
may coexist. A framing failure or `NeedMore` never advances the position. Terminal `rest`, terminal
arrays, and unknown enum bodies do not claim a leading extent and therefore cannot be consumed
ambiguously through these helpers.

## Recursive schemas

A closed selector enum may pass itself into a counted terminal array or an object body containing
[`wire::Recursive<T>`](crate::wire::Recursive). Recursive roots expose
`Schema::view::<DEPTH>(backing)`, where the caller chooses the const depth bound and zero produces
[`DepthExceeded`].

Generated root skipping uses one iterative typed continuation stack. Recursive array state retains
bounded exact geometry rather than item offsets. Proven fixed, affine, interval, palette,
factorized, recursive-shape, periodic, or packed-run representations accelerate lookup; unsupported
shapes fall back to exact prefix replay. Iteration always keeps one forward cursor and remains
linear in represented bytes.

Deriving [`WireBuilder`] generates progressive recursive object and array writers. One output cursor
moves through monomorphized continuation closures. The writer retains no recursive semantic tree,
encoded plan, allocation, dynamic dispatch, or hidden depth stack. Exact recursive views may be
copied directly.

See the runnable [recursive example](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.0/wire-repr/examples/recursive.rs).

## Manual capabilities

Manual formats implement reading and writing independently. [`WireView`] is an unsafe trait because
a manual implementation certifies that its reference-free state remains memory-safe for any
immutable span of the framed length. Generated parents validate child extents before invoking that
reconstruction boundary.

Manual writing remains safe:

```
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

A manual `FIXED_SIZE` allows later physical fields. Otherwise the child must be terminal or bounded
by an explicit byte controller. Manual writer errors may occur after partial output.

## Error contract

Generated errors are nominal field-site enums. Nested errors retain concrete source types. Read
errors carry absolute root-input offsets. Incomplete contiguous input reports [`NeedMore`] with the
offset and an exact or lower-bound `additional_at_least` value.

The caller owns buffering and retries. The core does not own `Read`, `AsyncRead`, segmented input,
or resumable parser state.

## Deliberate boundaries

The `1.0` core does not provide mutable views, semantic object materialization, runtime schema
reflection, negotiated selector registries, hidden collection indexes, async transport I/O, general
resource-limit machinery, or general traversal. These boundaries keep representation mechanics
static, allocation-free, and visible in the type system.
