# wire-repr

`wire-repr` provides exact byte-backed binary representations for Rust. Its
allocation-free codec contracts support fixed-width and prefix codecs, while
`wire_repr!` generates bounded views, constrained mutable views, and atomic
caller-buffer builders for sequential and fixed absolute-offset layouts.

This repository is a virtual two-crate workspace. `wire-repr/` is the public
`no_std` runtime facade and reexports `wire_repr!`; `wire-repr-macros/` is the
procedural-macro compiler used only at compile time. The workspace root owns
shared metadata, documentation, licensing, and toolchain settings. There is no
third support crate.

## Fixed layouts

Sequential layouts assign fields and anonymous spacing entries contiguous
one-based physical positions. Absolute layouts use zero-based byte offsets;
gaps are represented bytes and
are preserved verbatim, while overlapping codec extents are rejected before
input access. Both forms generate a borrowed view, typed structural errors,
exact and prefix parsing, byte access, and decoded getters. Fixed codecs decode
every exact-width bit pattern; domain validation remains consumer-owned. The
generated API has no runtime schema, allocation, or dynamic dispatch.

## Fixed byte spans

Use `bytes(N)` when a named field owns fixed-width bytes without interpreting
its contents:

```rust
wire_repr! {
    pub absolute layout DatabaseHeader {
        field magic: bytes(16) { offset: 0; }
    }
}
```

The getter returns the exact borrowed `&[u8]`. Consumers compare magic values,
inspect reserved bytes, or apply domain policy directly. Builders and setters
accept borrowed bytes and check only their exact width before mutation.

## Prefix fields

A sequential layout can mix fixed fields with custom `PrefixCodec`-backed
fields whose exact width is discovered by structural prefix parsing:

```rust
wire_repr::wire_repr! {
    pub layout Record {
        field kind: U8 { position: 1; }
        field name: prefix(crate::Terminated) { position: 2; }
        field checksum: BeU16 { position: 3; }
    }
}
```

The generated view retains only the represented bytes and one end boundary per
prefix field. `name()` decodes the exact accepted span, while
`name_encoded()` returns its original bytes, including legal noncanonical
encodings. Parsing validates each prefix once and safely rejects a custom codec
extent that exceeds the remaining input. Dynamic sequential views have no
compile-time `WIDTH`; their `as_bytes()` excludes the returned suffix.

## Bounded regions

A named field can own an opaque byte region whose length comes from an earlier
physical field:

```rust
wire_repr::wire_repr! {
    pub layout Frame {
        field payload_length: BeU16 { position: 1; }
        field payload: region(payload_length) { position: 2; }
    }
}
```

The length field may use a fixed or prefix codec, may be declared before or
after the region in source order, and may frame more than one later region. It
must physically precede every region that uses it. Its decoded value must
support checked conversion to `usize`; incompatible custom values produce a
normal Rust conversion-bound error, while values that do not fit produce the
layout's `InvalidRegionLength` parse error. A prefix length source is decoded
from its exact accepted encoding solely to establish framing. Other prefix
fields remain decode-free during parsing.

A region may be empty. Its getter returns the exact borrowed bytes without
semantic validation, reconstruction, or a duplicate encoded getter. Parse an
inner format explicitly with its own `parse_exact` when needed. Regions store
only an exclusive end boundary, remain part of `as_bytes()`, and work with
later fixed fields, prefix fields, padding, alignment, and other independently
bounded regions. They are sequential-only and cannot own projections or serve
as another region's length source.

## Padding and alignment

Sequential layouts can include anonymous physical spacing entries:

```rust
wire_repr::wire_repr! {
    pub layout Record {
        field kind: U8 { position: 1; }
        padding { position: 2; length: 3; }
        align { position: 3; boundary: 8; }
        field payload: prefix(crate::Terminated) { position: 4; }
    }
}
```

Positions remain contiguous across fields, padding, and alignment entries.
Padding consumes its fixed nonzero length. Alignment consumes the minimum bytes
needed to place the next entry at a multiple of its nonzero boundary relative
to the start of the represented layout; boundary `1` is a valid no-op. Spacing
bytes are opaque, have no generated getters, remain part of `as_bytes()`, and
are never normalized during parsing. Use a `bytes(N)` field when reserved bytes
need a name or direct inspection; consumers own their semantics. Fixed sequential layouts
retain `WIDTH`; prefix-backed layouts compute spacing from the validated runtime
offset. Absolute layouts use explicit offsets and reject these entries.

## Bit projections

A fixed unsigned builtin storage field can expose read-only named bit projections:

```rust
wire_repr::wire_repr! {
    pub layout Header {
        field flags: U8 { position: 1; projections {
            /// Whether processing is enabled.
            bit enabled: 0;
            /// Normalized mode bits.
            bits mode: 1..=3;
        } }
    }
}
```

Bit zero is the decoded unsigned value's LSB, irrespective of wire endianness.
`U8`, unsigned 16/24/32/64/128-bit builtins are eligible; signed and custom
codecs are not. Storage remains the only byte owner and is still readable via
its getter. Projection getters are immutable direct shift/mask operations with
no validation, metadata, allocation, or runtime dispatch.

## Mutation and building

Generated mutable views retain the same represented prefix and dynamic
boundaries as immutable views. Fixed fields have typed same-width setters when
changing them cannot alter region framing. Prefix fields, regions, and region
length sources have no in-place setters. There is no unrestricted mutable-byte
access, so padding, alignment bytes, gaps, and returned suffixes cannot be
changed accidentally through a view.

Builders plan every codec and compute the complete represented extent before
writing to caller-owned output. Every returned error leaves the entire output
unchanged. A successful build returns a bounded mutable view and its disjoint
mutable suffix without reparsing. Dynamic builders accept region byte slices and
derive each region length source automatically; regions sharing one source must
have equal lengths. A source value must support checked construction from
`usize`, and its codec's documented encode/decode round-trip law must preserve
that length.

Repeated sequences are not supported. Prefix fields and bounded regions remain
sequential-only; absolute layouts remain fixed-width.

## Contract

- Rust 1.91.0, edition 2024.
- Library targets are `no_std` and `no_alloc`.
- Safe Rust only; `unsafe_code` is denied.
- No target-runtime dependencies.
- Default features are empty.

## License

MIT. See [LICENSE](LICENSE).
