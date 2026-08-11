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
        field payload: region(payload_length);
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
layouts are fixed-width and do not support prefix fields, regions, padding, or alignment.

# Field forms

The supported field forms are:

```text
field name: U8;
field name: BeU16;
field name: codec(path::ToFixedCodec);
field name: bytes(N);
field name: prefix(path::ToPrefixCodec);
field name: region(length_field);
```

The built-in integer names are `U8`, `I8`, the `Be`/`Le` signed and unsigned
16/32/64/128-bit codecs, and the unsigned `BeU24`/`LeU24` codecs. Custom `codec(path)`
fields implement `wire_repr::FixedCodec`; `prefix(path)` fields implement
`wire_repr::PrefixCodec`.

`bytes(N)` is a fixed-width opaque borrowed span. `region(source)` is a dynamic opaque
borrowed span framed by an earlier physical fixed or prefix field. A builder derives the
source value from the region length, so the source does not receive a fluent input method.
If multiple regions share one source, their lengths must agree.

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
the sole physical byte owner.

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

- `PacketView<'wire>` with `parse_prefix`, `parse_exact`, `as_bytes`, and field getters;
- `PacketViewMut<'wire>` with `parse_prefix_mut`, `parse_exact_mut`, `as_bytes`, `as_view`,
  `into_view`, and eligible fixed-field setters;
- `PacketBuilder<'value>` with `new`, fluent field inputs, and `build_into`;
- `PacketError`, `PacketMutationError`, and `PacketWriteError`.

Fixed layouts also expose `PacketView::WIDTH`. Prefix fields generate both `field()` for the
decoded value and `field_encoded()` for the exact accepted wire encoding. A bit projection
generates a getter with the projection name.

A successful prefix parser or builder returns the bounded representation and a disjoint
suffix. Exact parsing rejects trailing bytes. Setters and builders plan every fallible
encoding operation before mutation; failed operations preserve caller-owned output.

Generated items inherit the layout visibility. Layout and field documentation attributes
are preserved on their generated API owners.

# Compile-time validation

The macro rejects malformed or ambiguous layouts before generated code is type-checked,
including zero, duplicate, or gapped explicit positions; mixed placement modes; invalid
absolute offsets; overlapping projections; generated-name collisions; invalid region
sources; unsupported field forms; and arithmetic overflow in statically known extents.

Repeated sequences, tagged unions, arbitrary conditional fields, and inferred absolute
offsets are deliberately outside this macro's grammar.
