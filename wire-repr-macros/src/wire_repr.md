# `wire_repr!`

Declares a concrete byte-backed layout. The macro emits borrowed immutable views,
restricted mutable views, builders, and layout-specific errors; it emits neither a
runtime descriptor nor a Rust-struct reinterpretation of input bytes. Import it from
the facade:

```rust,ignore
use wire_repr::wire_repr;
```

A declaration owns physical representation only. Application code still owns magic
values, reserved-byte policy, checksums as protocol rules, and cross-field meaning.

## Basic syntax

```text
[attributes]
visibility scalar ScalarName: Codec;

[attributes]
visibility layout Name {
    context name: Type;
    field name: FieldForm [as SemanticType] { properties }
    padding { properties }
    align { properties }
}

[attributes]
visibility absolute layout Name {
    field name: FixedFieldForm [as SemanticType] { offset: N; }
}
```

Top-level `scalar` declarations and layout-local `context` declarations are optional.
A sequential layout contains fields, padding, and alignment. An absolute layout
contains fixed fields only.

## Sequential and absolute geometry

A sequential layout has exactly one placement mode:

- **Implicit:** no physical entry has `position`; physical order is declaration order.
- **Explicit:** every field, `padding`, and `align` entry has a unique contiguous
  one-based `position`; physical order may differ from declaration order.

Declaration order remains the order of getters, setters, builder inputs,
missing/planning errors, and rustdoc. Physical order controls parsing, represented
bytes, dynamic progress, builder commit order, and physical-layout errors.

```rust,ignore
use wire_repr::wire_repr;

wire_repr! {
    pub layout Reordered {
        field checksum: BeU16 { position: 2; }
        field tag: U8 { position: 1; }
    }
}

let bytes = [7, 0x12, 0x34];
let view = Reordered::view(&bytes).without_trailing()?;
assert_eq!(view.checksum(), 0x1234);
assert_eq!(view.tag(), 7);
# Ok::<(), ReorderedError>(())
```

`padding { length: N; }` consumes a fixed nonzero opaque span.
`align { boundary: N; }` consumes the minimum bytes needed to align the current
position relative to representation byte zero. In explicit mode use
`padding { position: P; length: N; }` and `align { position: P; boundary: N; }`.
Builders preserve spacing bytes already in their destination.

An `absolute layout` requires a zero-based `offset` on every field. Its width is the
largest field extent; gaps are represented bytes and builders preserve existing gap
contents. Offsets are never inferred. Absolute layouts reject padding, alignment,
prefix fields, and dynamic ranges; invalid codec widths, overflowing extents, and
overlap are rejected before input slicing.

```rust,ignore
use wire_repr::wire_repr;

wire_repr! {
    pub absolute layout FileHeader {
        field magic: bytes(4) { offset: 0; }
        field version: BeU16 { offset: 8; }
    }
}

let bytes = [0; 10];
let header = FileHeader::view(&bytes).without_trailing()?;
assert_eq!(header.magic(), &[0; 4]);
assert_eq!(header.version(), 0);
# Ok::<(), FileHeaderError>(())
```

## Field forms

```text
field name: U8;
field name: BeU16;
field name: BeU16 as path::Semantic;
field name: codec(path::FixedCodec);
field name: bytes(N);
field name: bytes(N) as path::Semantic;
field name: prefix(path::PrefixCodec);
field name: bytes(current_pos..current_pos + source);
field name: bytes(current_pos..source);
field name: bytes(current_pos..buf_end);
```

Built-ins are `U8`, `I8`, unsigned `Be`/`Le` 16/24/32/64/128-bit codecs, and signed
`Be`/`Le` 16/32/64/128-bit codecs. `codec(path)` implements `wire_repr::FixedCodec`;
`prefix(path)` implements `wire_repr::PrefixCodec`. `bytes(N)` is a fixed, opaque,
borrowed span and requires `N > 0`.

A prefix codec validates its bounded encoded extent while parsing. The generated
`field()` decodes that exact span; `field_raw()` returns the original accepted bytes,
including legal noncanonical encodings. A prefix's width is structural information,
not a source for a dynamic range.

## Dynamic byte ranges

Ranges are sequential-only opaque borrowed spans:

- `bytes(current_pos..current_pos + source)` uses a source as a relative length.
- `bytes(current_pos..source)` uses a source as an exclusive endpoint from
  representation byte zero.
- `bytes(current_pos..buf_end)` owns the supplied view buffer tail.

The first two require an eligible physically preceding built-in fixed integer or a
total mapping over one. Parsing uses the raw physical integer and checked `usize`
conversion; mappings do not change geometry. Prefix, custom/direct, declared-scalar,
nominal, and range fields cannot be sources. A dynamic range may be empty.

For builders, relative sources derive payload length. Absolute sources derive the
physical exclusive end, including earlier fixed/prefix widths, padding, alignment,
and ranges. A derived source has no ordinary builder input or setter. Shared sources
must derive the same value under the same algebra. `buf_end` has no source, may occur
once, and must be physically last. It makes `with_remainder()` return an empty suffix;
it does not discover an external packet boundary. Mutable views expose each range as
`field_mut()`, an exact validated span that may be changed but cannot resize or reframe.

## Mappings, scalars, and projections

`as TypePath` is a total nominal mapping placed immediately after an eligible built-in
fixed integer or `bytes(N)` field form, before the field body:

```rust,ignore
use wire_repr::wire_repr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Kind(u16);
impl From<u16> for Kind { fn from(value: u16) -> Self { Self(value) } }
impl From<Kind> for u16 { fn from(value: Kind) -> Self { value.0 } }

wire_repr! {
    pub layout Mapped {
        field kind: BeU16 as Kind;
    }
}

let view = Mapped::view(&[0, 9]).without_trailing()?;
assert_eq!(view.kind(), Kind(9));
assert_eq!(view.kind_raw(), 9);
# Ok::<(), MappedError>(())
```

Mappings require `Semantic: From<Raw>` and `Raw: From<Semantic>`. `Raw` is the exact
physical type (`u32` for U24 and `[u8; N]` for fixed bytes). Mapped byte values are
owned arrays or wrappers; unmapped `bytes(N)` remains borrowed. Mapped fields have
semantic and raw getters; eligible mutable fields have semantic and raw setters; the
two builder input methods share one slot and the last call wins. A mapped range source
has both getters but no setter or builder input because its raw value is derived.

Mappings are not supported on `scalar`, custom, prefix, or range fields. Instead,
`scalar Name: Codec;` declares a reusable nominal wrapper owning that codec.

Unsigned built-in integer storage can have immutable LSB0 projections:

```rust,ignore
field flags: U8 {
    projections {
        bit enabled: 0;
        bits mode: 1..=3;
    }
}
```

`bit` returns `bool`; `bits` returns the storage scalar after shifting the inclusive
range down to bit zero. Bit numbering follows the decoded raw integer regardless of
wire endianness. Projection ranges on one field cannot overlap. Projections on mapped
integer storage still use raw physical storage; signed and custom fields have none.

## Contexts, derivation, and finalization

A `context name: Type;` appears before all physical entries. It is a builder-only
borrowed input (`&'value Type`, including unsized types), not bytes or parser state.
Contexts can have documentation attributes but no visibility.

A fixed field can be computed before writing:

```text
field total: BeU16 {
    derive: path(value(other_field), len(payload));
    derive_error: path::Error;
}
```

`derive` calls a static function returning `Result<FieldSemanticValue, DeclaredError>`.
Its operands are `value(field)` and `len(range)`. Dependencies must be known, valid,
acyclic, and non-self-referential. Derived fields have no ordinary input or setter;
the builder evaluates them in deterministic topological order during preflight.

A `finalize` field is an infallible post-write patch:

```text
field checksum: BeU16 {
    finalize: path(bytes(buf_start..buf_end), context(name), value(field));
}
```

Finalizer operands are `bytes(boundary..boundary)`, `context(name)`, and
`value(field)`. Byte boundaries are `buf_start`, `buf_end`, `field.start`, and
`field.end`. A field uses `derive` or `finalize`, never both. Finalizer targets are
only direct, unmapped, unprojected built-in fixed integers with infallible encoding;
`BeU24` and `LeU24` are excluded. The target starts zeroed, finalizers run in stable
compile-time DAG order, and their return types must exactly equal the target semantic
type. `buf_end` is the represented extent, not destination capacity.

## Generated surface and framing

For `layout Packet`, the macro generates `Packet<'wire>`, `PacketViewMut<'wire>`,
`PacketBuilder<'value>`, `PacketError`, `PacketMutationError`, and `PacketWriteError`.
Fixed layouts also have `Packet::WIDTH`. Layout and field documentation attributes
are copied to their generated API owners, and generated items inherit layout visibility.

`Packet::view(bytes)` is a request only. Call exactly one terminal:
`with_remainder()` returns `(Packet<'wire>, &'wire [u8])`; `without_trailing()`
rejects a suffix. A terminal structurally parses once. Dynamic immutable views retain
validated endpoints instead of recomputing them in getters. `as_bytes()` is exactly
the representation, excluding any `with_remainder` suffix.

Mutable parsing is separate: `PacketViewMut::parse_prefix_mut` permits a suffix and
`parse_exact_mut` rejects one. Mutable views retain the same accepted extent and
boundaries as immutable parsing. Typed setters exist only for same-width fixed fields
that cannot change framing; range sources, prefixes, dynamic ranges, and `buf_end`
have no setter.

Builders use `new`, fluent inputs, and `build_into`. All fallible inputs, codec plans,
derivations, conversion, geometry, arithmetic, and capacity checks finish before the
first caller-output mutation. Successful builds return the bounded mutable view and a
disjoint untouched suffix; any build error leaves the whole supplied output unchanged.

## Compile-time rejection

The macro rejects malformed declarations and structural ambiguity before generated
methods are type-checked. This includes mixed, duplicate, zero, or gapped explicit
positions; missing or invalid absolute offsets; overlapping projections; generated-name
collisions; invalid range sources; unsupported field forms; invalid static extents;
and invalid derive/finalizer dependencies. Repeated sequences, tagged unions,
arbitrary conditional fields, nested range schemas, and inferred absolute offsets are
outside the grammar.

```rust,ignore
use wire_repr::wire_repr;

wire_repr! {
    pub layout MixedPlacement {
        field first: U8 { position: 1; }
        field second: U8;
    }
}
```
