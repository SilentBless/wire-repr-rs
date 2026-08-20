Declares sequential or fixed absolute-offset byte-backed layouts.

Consumers normally import this macro from the facade:

```rust,ignore
use wire_repr::wire_repr;
```

The declaration compiles to concrete borrowed views, constrained mutable views, builders,
and layout-specific errors. It does not create runtime descriptors or reinterpret input as
Rust structs.

# Sequential layouts

Sequential layouts support either fully implicit or fully explicit physical placement.
With implicit placement, fields, padding, and alignment occupy contiguous one-based
positions in declaration order:

```rust,ignore
wire_repr! {
    pub layout Packet {
        field kind: U8;
        padding { length: 2; }
        align { boundary: 4; }
        field payload_length: BeU16;
        field payload: bytes(current_pos..current_pos + payload_length);
    }
}
```

To reorder physical storage independently of declaration and generated API order, every
physical entry must provide `position`:

```rust,ignore
wire_repr! {
    pub layout Reordered {
        field checksum: BeU16 { position: 2; }
        field tag: U8 { position: 1; }
    }
}
```

Positions are one-based, unique, contiguous, and shared by fields, padding, and alignment.
Mixing explicit and implicit entries is rejected.

# Absolute layouts

Absolute layouts require an explicit zero-based `offset` for every field:

```rust,ignore
wire_repr! {
    pub absolute layout FileHeader {
        field magic: bytes(4) { offset: 0; }
        field version: BeU16 { offset: 8; }
    }
}
```

Gaps are represented and preserved. Overlapping field extents are rejected. Absolute
layouts are fixed-width and do not support prefix fields, byte ranges, padding, or alignment.

# Field forms

The supported field forms are:

```text
field name: U8;
field name: BeU16;
field name: BeU16 as path::Semantic;
field name: codec(path::ToFixedCodec);
field name: bytes(N);
field name: bytes(N) as path::Semantic;
field name: prefix(path::ToPrefixCodec);
field name: bytes(current_pos..current_pos + source);
field name: bytes(current_pos..source);
field name: bytes(current_pos..buf_end);
```

The built-in integer names are `U8`, `I8`, the `Be`/`Le` signed and unsigned
16/32/64/128-bit codecs, and unsigned `BeU24`/`LeU24`. Custom `codec(path)` fields
implement `wire_repr::FixedCodec`; `prefix(path)` fields implement `wire_repr::PrefixCodec`.

`bytes(N)` is a fixed-width opaque borrowed span. Byte ranges are dynamic opaque borrowed
spans: `current_pos + source` is a relative length, `source` is an exclusive absolute
endpoint from representation byte zero, and `buf_end` consumes the supplied view-buffer
tail. The first two require a physically preceding built-in fixed integer or semantic mapping
over one. Geometry uses the raw physical integer and checked conversion to `usize`; prefix,
custom/direct, declared scalar, nominal, and byte-range sources are unsupported. `bytes(0)`
is invalid, but dynamic ranges may be empty.

Builders derive relative lengths or absolute physical endpoints; absolute derivation includes
preceding fixed/prefix widths, padding, alignment, and ranges. Derived sources have no
fluent input or setter. Shared sources require identical derived values under the same
algebra. `buf_end` has no source, may occur once, and must be physically last. Its
`with_remainder` suffix is empty; other `with_remainder` suffixes follow the complete represented
layout, including physical entries after an absolute range. A mutable view exposes each range
as `field_mut()`, which may change bytes but cannot resize or reframe the range. Unsupported
variable-width-source framing, such as ULEB128 section lengths, remains consumer-owned.



# Builder contexts and post-write finalizers

A layout may declare generated-builder-only borrowed inputs before every physical field,
padding, or alignment entry:

```rust,ignore
pub layout UdpPacket {
    /// Transport pseudo-header used only while building.
    context pseudo: crate::PseudoHeader;
    field checksum: BeU16 {
        finalize: crate::udp_checksum(
            bytes(buf_start..buf_end),
            context(pseudo),
        );
    }
    field payload: bytes(current_pos..buf_end);
}
```

A context names its referent type rather than a reference; the generated builder will borrow it
as `&'value T` (including unsized `T`, such as `[u8]`). Contexts accept documentation
attributes but no visibility: they are not independent public items, encoded bytes, view state,
or parser state. Every external input is explicit—there is no ambient finalizer state.

`finalize: path(operand, ...);` is an infallible post-write field property. Its operands are
`bytes(boundary..boundary)`, `context(name)`, and `value(field)`. `value(field)` passes the
field's semantic value; it may name an ordinary field, an explicit pre-write-derived field, or an
earlier finalizer. Only a finalized source creates post-write finalizer ordering. Finalizer byte
boundaries are only `buf_start`, `buf_end`, `field.start`, and `field.end`; `current_pos`,
arithmetic, and raw source identifiers are not valid there. A field uses either fallible pre-write
`derive` (paired with `derive_error`) or infallible post-write `finalize`, never both.

Before finalization, every target is zeroed. A finalizer may include its own target in a
`bytes(...)` operand and observes those zero bytes, but `value(its_own_target)` is invalid. Finalizer
dependencies form a compile-time DAG, and every fallible plan or capacity check completes before
writing starts. Finalizers return their target's exact semantic integer type directly, so patching is
infallible after commit. Generated-method type checking rejects a return type that does not
exactly match the target's semantic integer type.

The initial target set is unmapped built-in fixed integers except `BeU24`
and `LeU24`: their `u32` semantic domain requires a fallible 24-bit representability check. Custom
patch targets are not supported.

`buf_end` means the end of the represented layout, not the capacity of the caller-provided output
buffer. A builder may receive a larger destination, but finalizer ranges are bounded by the
representation it constructs.


# Pre-write derived fields

A fixed-codec field may be builder-derived instead of supplied as a fluent input:

```rust,ignore
field total: BeU16 {
    position: 1;
    derive: crate::derive_total(len(options), len(payload));
    derive_error: crate::LengthError;
}
```

`derive` is a direct static function call returning `Result<FieldSemanticValue, DeclaredError>`.
Its only operands are `value(field)`, which passes the referenced semantic fixed-field value by
reference, and `len(range)`, which passes the borrowed or `_existing(length)` range input length
as `usize`. The macro rejects unknown, self, cyclic, or wrong-kind dependencies. Explicit derived
fixed fields are evaluated in a deterministic topological order (source declaration order breaks
ties), then planned before any destination mutation; their typed failure appears as a dedicated
`PacketWriteError::DeriveField…(LengthError)` variant. Its display text identifies the failed
field without formatting the declared payload. The write error always implements `Debug` and
`Error`: generated `Debug` reports the exact derivation variant while marking its payload opaque,
so declared derivation errors need not implement `Debug`, `Display`, or `Error`. Because a public
layout publishes its write error and typed variants, its declared derivation error must be public
enough to appear in that API. They cannot frame byte ranges in this stage.

This is pre-write input derivation, not checksum/CRC finalization: it cannot inspect output bytes,
run after a write, or repair a partially written destination.

# Total semantic mappings

`as TypePath` maps one eligible physical field to a nominal consumer type. It appears
immediately after the built-in fixed integer codec or `bytes(N)` and before a field body
containing `position`, `offset`, or `projections`. It is not permitted on declared scalar
codecs, `codec(path)` fields, prefix fields, or byte ranges.

The physical codec remains the wire owner. Mapping is total and direct: the semantic getter
uses `Semantic: From<Raw>`; semantic setters and builders use `Raw: From<Semantic>`. Raw is
the codec's exact type (`u32` for `U24`, `[u8; N]` for `bytes(N)`), not a narrowed or
validated domain value. There is no fallible conversion layer. Thus physical encoding can
still reject a raw value (for example, an out-of-range `U24`) and preserves its usual
whole-destination atomicity. Mapped bytes are owned arrays/wrappers; unmapped `bytes(N)`
returns its borrowed slice.

Top-level `scalar Name: Codec;` is separate syntax that declares a reusable nominal wrapper
owning a codec. It does not make that codec eligible for `as TypePath` mapping.

# Projections

Unsigned built-in storage fields may declare immutable semantic projections:

```rust,ignore
field flags: U8 {
    projections {
        bit enabled: 0;
        bits mode: 1..=3;
    }
}
```

Projection numbering is semantic LSB0 after endian decoding. `bit` generates a `bool`
getter; `bits` shifts the inclusive range down to bit zero and returns the storage scalar
type. Projection ranges on one storage field may not overlap. The storage field remains
the sole physical byte owner. On a mapped integer storage field, projections continue to
operate on that physical decoded raw integer.

When a sequential field uses both explicit placement and projections, both properties are
inside the same field body:

```rust,ignore
field flags: BeU16 {
    position: 2;
    projections {
        bit urgent: 15;
    }
}
```

# Spacing

Sequential layouts may contain opaque represented spacing:

```text
padding { length: N; }
align { boundary: N; }
padding { position: P; length: N; }
align { position: P; boundary: N; }
```

Padding advances by a fixed number of bytes. Alignment advances to the next offset aligned
to the declared boundary. Builders preserve existing spacing bytes in caller output rather
than normalizing them.

# Generated names and methods

For a declaration named `Packet`, the macro generates:

- `Packet<'wire>` with `view(bytes).with_remainder()`,
  `view(bytes).without_trailing()`, `as_bytes`, and field getters;
- `PacketViewMut<'wire>` with `parse_prefix_mut`, `parse_exact_mut`, `as_bytes`, `as_view`,
  `into_view`, eligible fixed-field setters, and mutable byte-range accessors;
- `PacketBuilder<'value>` with `new`, fluent field inputs, and `build_into`;
- `PacketError`, `PacketMutationError`, and `PacketWriteError`.

Fixed layouts also expose `Packet::WIDTH`. Prefix fields generate both `field()` for the
decoded value and `field_raw()` for the exact validated raw wire bytes (the original wire
representation). A bit projection
generates a getter with the projection name. A mapped field generates `field()` for its
semantic type and `field_raw()` for its raw codec type. If mutable, it generates both
`set_field(semantic)` and `set_field_raw(raw)`. Its builder generates `field(semantic)` and
`field_raw(raw)`; both set the same slot, so the last call wins. A mapped byte-range
source has both getters but no setter or builder input because its raw value is derived from
the byte range.

A successful `with_remainder` terminal or builder returns the bounded representation and a
disjoint suffix. `without_trailing` rejects trailing bytes. Setters and builders plan every fallible
encoding operation before mutation; failed operations preserve caller-owned output.

Generated items inherit the layout visibility. Layout and field documentation attributes
are preserved on their generated API owners.

# Compile-time validation

The macro rejects malformed or ambiguous layouts before generated code is type-checked,
including zero, duplicate, or gapped explicit positions; mixed placement modes; invalid
absolute offsets; overlapping projections; generated-name collisions; invalid byte range
sources; unsupported field forms; and arithmetic overflow in statically known extents.

Repeated sequences, tagged unions, arbitrary conditional fields, and inferred absolute
offsets are deliberately outside this macro's grammar.
