# Exact-source wire views and progressive writers

`wire-repr` compiles Rust schema declarations into immutable views over exact source bytes and
output-owning typestate writers. The generated target API is `no_std`, allocation-free, and
statically dispatched.

A schema describes physical representation rather than a decoded semantic object. The library owns
byte order, widths, field order, framing, selectors, controllers, geometry, and exact represented
ranges. Protocol and application semantics remain in consumer code.

## Installation

```toml
[dependencies]
wire-repr = "1"
thiserror = { version = "2", default-features = false }
```

Generated nominal error types use `thiserror`'s derive contract, so schema crates declare it
directly. Both dependencies remain `no_std`.

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

## Executable showcases

The repository includes live network examples with no application credentials:

- [DNS over UDP](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.1/wire-repr/examples/dns.rs)
  builds a query and frames the returned datagram.
- [NTPv4 over UDP](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.1/wire-repr/examples/ntp.rs)
  validates real server timestamps and calculates the observed clock offset.
- [Telegram MTProto over TCP](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.1/wire-repr/examples/telegram.rs)
  performs `req_pq_multi → resPQ` against DC2 with typed TL views, computed request fields, schema
  validators, bounded transport reads, and a runtime fingerprint array.
- The [recursive expression VM](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.1/wire-repr/examples/expression_vm.rs)
  compiles an expression, patches a physical digest, validates the framed program, and evaluates
  retained recursive child views.

Network and operating-system randomness are example-only dependencies; they do not enter the
normal target graph.

DNS and NTP are plaintext demonstrations: their correlation fields do not authenticate a responder.
The Telegram example likewise stops before the authentication-key exchange establishes identity.


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

Direct scalar fields support fixed-width Rust integers and floats. Fixed multibyte scalars require
`be` or `le`; one-byte integers do not accept an endian marker. Primitive fixed arrays use the same
per-element representation:

```
use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Lanes {
    #[wire(le)]
    lanes: [u16; 8],
}
```
Rust `f32` is IEEE 754 binary32 (usually called `float`); `f64` is binary64 (`double`).
Both support `be`, `le`, and fixed arrays. Reading and writing preserve their bits, including NaN
payloads, infinities, and signed zero; compare `to_bits()` when those distinctions matter.

```
use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct Measurement {
    #[wire(le)]
    value: f32,
    #[wire(be)]
    time: f64,
    #[wire(le)]
    samples: [f32; 4],
}
```


Platform-sized and logical types declare an explicit physical integer representation:

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

### Variable-width integers

Integer fields may select a byte encoding instead of an endian marker:

| Attribute | Physical type | Format |
| --- | --- | --- |
| `uleb128` | `u8` through `u128` | Unsigned LEB128; also Protobuf unsigned varint |
| `sleb128` | `i8` through `i128` | Signed LEB128 with terminal sign extension |
| `uleb128, zigzag` | `i8` through `i128` | ZigZag followed by unsigned LEB128 |
| `vu128` | `u8` through `u128` | VU128 prefix-length integer |
| `vu128, zigzag` | `i8` through `i128` | ZigZag followed by VU128 |

`zigzag` is a signed-to-unsigned transformation, not a standalone byte format.
`varint` and `prefix_varint` are not attribute aliases: those names describe incompatible protocol
families. In particular, SLEB128 and ZigZag are not substitutes for a protocol's signed
two's-complement varint. VU128 follows the current format, not its superseded original version.

The `as = integer_type` conversion and stored constants compose with these encodings. Unsigned
primitive ULEB128 and VU128 fields can control `bytes` or `counted_by`. Their setters remain omitted.
Writers reserve one byte for a deferred controller and expand its prefix in the same output after
the payload completes, without staging or hidden allocation. Prefixes expand in reverse physical
order before computed callbacks run; a fixed buffer need only fit the final representation.

For example, a WebAssembly custom section carries a ULEB128 content length and a UTF-8 name
represented here as bytes:

```
use wire_repr::{WireBuilder, WireView, wire};

#[derive(WireView, WireBuilder)]
struct CustomContent {
    #[wire(uleb128)]
    name_length: u32,
    #[wire(bytes = name_length)]
    name: wire::Bytes,
    #[wire(rest)]
    data: wire::Bytes,
}

#[derive(WireView, WireBuilder)]
struct CustomSection {
    #[wire(constant = 0)]
    id: u8,
    #[wire(uleb128)]
    size: u32,
    #[wire(bytes = size)]
    content: CustomContent,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut output = [0; 8];
    let written = CustomSection::builder(&mut output[..])
        .content(|content| content.name(&b"note"[..]).data(&[0x2a][..]))?
        .finish()?;
    assert_eq!(written.as_bytes(), &[0, 6, 4, b'n', b'o', b't', b'e', 0x2a]);
    let section = CustomSection::view(written.as_bytes())?;
    assert_eq!(section.size(), 6);
    assert_eq!(section.content().name(), b"note");
    Ok(())
}
```

Views accept complete nonminimal encodings that fit the physical width and preserve their exact
bytes. LEB128 is bounded by `ceil(bits / 7)` bytes; VU128 has a prefix-declared extent of at most
seventeen bytes, including nonminimal narrow values. Fresh writes emit the shortest form.
Malformed fields report [`Uleb128Error`], [`Sleb128Error`], or [`Vu128Error`] at their field site;
VU128 truncation reports the exact missing encoded bytes once its prefix is available.

Variable encodings are not supported for fixed primitive arrays, enum selectors, bitfield cells or
projections, conditional or computed destinations, placement controllers, or recursive body
grammars. Alignment or forward placement following a deferred variable controller is rejected
because later prefix expansion would invalidate that placement.

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

Count controllers accept unsigned integers, native `i8`, `i16`, `i32`, `i64`, and `i128`, and
logical `isize` with an explicit signed `as = integer_type` representation. One-byte `i8` needs
no endian marker; wider fixed representations require `le` or `be`. SLEB128 and ZigZag with
ULEB128 or VU128 also support signed counts outside recursive body grammars.

Negative counts fail with the field-site `CountValue(ScalarConversionError)` from the physical
signed type to `usize` at the controller's absolute input offset before array geometry, even
through `view_unchecked()`. Physical-to-`isize` conversion can separately fail. A zero count is an
empty array; `-1` is an error, not `null`. Both writer paths derive the number of emitted items and
check the physical representation and logical type ranges. Byte-length, placement, and
bit-projection controllers remain unsigned.

Fixed recursive counted arrays accept the same signed controllers while retaining their existing
compact `u32` continuation and count boundary. Negative counts fail before any child resumes;
schema-specific conversion errors flatten to `RecursiveError::Child`. Recursive variable-integer
controllers remain unsupported.

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

Nested struct and enum-body closures receive detached builders. Their array setters accept
`array.extend(source, |item, value| ...)`, retaining the `IntoIterator` source and callback by value
without traversing them. The enclosing write consumes the source once and emits each item directly
before patching the stored count. Detached setters remain infallible; item, count, and output
errors propagate through the enclosing write. Borrowed sources must remain valid until that write.
No item plans or hidden allocation are introduced, including for multiple nested array levels.

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
Callbacks with arguments use the generated `WireView` to resolve exact fields and ranges. A
zero-argument callback needs no view and remains available to a `WireBuilder`-only schema.

The
[expression VM](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.1/wire-repr/examples/expression_vm.rs)
uses `exclude(self)` to compute an FNV-1a digest over dynamic bytecode, then verifies the stored
value through a schema validator.

## Sequences and cursors

Syntactically fixed schemas expose prevalidated [`FixedViews`], an infallible
`ExactSizeIterator`. Variable schemas with a provable leading extent expose [`VariableViews`], whose
`next` frames one representation lazily. [`Cursor`] consumes heterogeneous schemas from one shared
input.

Views yielded by these facades borrow the original input rather than the facade or cursor, so they
may coexist. A framing failure or `NeedMore` never advances the position. A terminal counted array
has a leading extent when its items do: prefix framing locates the counted boundary, including a
zero count. An exact standalone view retains deferred terminal-item validation. Terminal `rest`
and unknown enum bodies cannot be consumed ambiguously through these helpers.

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

See the runnable
[recursive expression VM](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.1/wire-repr/examples/expression_vm.rs).

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

A nonterminal read child may be fixed-width, explicitly byte-bounded, or self-delimiting through
[`WireView::LEADING_EXTENT`]. In the last case the parent frames it once and retains its state and
the required boundaries; getters do not reframe it. A child without any such boundary remains
terminal-only unless an explicit byte controller bounds it.

Writers use the actual bytes emitted by a variable child before writing subsequent fields. This
works with [`WireBuilder`] and [`WireWrite`] alone; no read capability or preliminary encoding pass
is required. Progressive setters whose offsets depend on a variable child must follow physical
order; calling them before the necessary earlier setters returns a field-site [`LayoutError`].
Fixed-size instantiations keep their static offsets and permit reordered setters. Manual writer
errors may occur after partial output.

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
