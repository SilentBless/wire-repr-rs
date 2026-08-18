# Zero-cost byte-backed representations

`wire-repr` generates safe borrowed views, constrained mutable views, and atomic
caller-buffer builders for binary layouts. Generated code works directly with byte
slices: it does not cast bytes to Rust structs, allocate, perform I/O, or depend on
alignment and ABI layout.

The crate is `no_std`, has no default features, and keeps domain policy in the
consumer. Layouts establish byte ownership, bounds, framing, and encoding mechanics;
callers remain responsible for magic values, reserved bits, checksums, and cross-field
semantics.

## Quick start

```rust
use wire_repr::wire_repr;

wire_repr! {
    pub layout Header {
        field kind: U8;
        field length: BeU16;
        field flags: U8 {
            projections {
                bit enabled: 0;
                bits mode: 1..=3;
            }
        }
    }
}

let input = [7, 0x01, 0x00, 0b0000_1011, 0xff];
let (view, suffix) = HeaderView::parse_prefix(&input).unwrap();

assert_eq!(view.as_bytes(), &input[..4]);
assert_eq!(suffix, &[0xff]);
assert_eq!(view.kind(), 7);
assert_eq!(view.length(), 256);
assert!(view.enabled());
assert_eq!(view.mode(), 5);

let mut output = [0u8; 5];
let (mut view, suffix) = HeaderBuilder::new()
    .kind(7)
    .length(256)
    .flags(0b0000_1011)
    .build_into(&mut output)
    .unwrap();

view.set_kind(8).unwrap();
assert_eq!(view.as_bytes(), &[8, 0x01, 0x00, 0b0000_1011]);
assert_eq!(suffix, &[0]);
```

## Parsing and represented bytes

Every generated immutable view provides two explicit entry points:

- `parse_prefix` parses one representation and returns the disjoint suffix;
- `parse_exact` requires the complete input to be exactly one representation.

`as_bytes` returns only the bytes owned by the representation. Accepted bytes are
borrowed and preserved verbatim, including opaque gaps and legal noncanonical encodings
accepted by a custom [`PrefixCodec`]. Parsing performs structural validation; it does
not reconstruct bytes from decoded values or apply consumer policy.

Fixed layouts expose `View::WIDTH`. Dynamic sequential layouts discover their represented
extent while validating prefix fields, byte ranges, padding, and alignment.

## Generated API

For `layout Packet`, [`wire_repr!`] generates a family centered on the declaration:

| Generated item | Purpose |
| --- | --- |
| `PacketView<'wire>` | Immutable borrowed representation and field getters |
| `PacketViewMut<'wire>` | Constrained mutable representation and eligible setters |
| `PacketBuilder<'value>` | Fluent caller-buffer construction |
| `PacketError` | Structural parse errors |
| `PacketMutationError` | Fixed-field planning and setter errors |
| `PacketWriteError` | Missing inputs, planning, extent, and capacity errors |

A field getter has the declared field name. Prefix fields also expose
`<field>_raw()` so callers can inspect the exact validated raw wire bytes (the original
wire representation). Bit projections
produce named `bool` or unsigned scalar getters without becoming independent byte owners.

Mutable views provide `parse_prefix_mut`, `parse_exact_mut`, `as_view`, and `into_view`.
Setters are generated only for same-width fields whose mutation cannot invalidate dynamic
framing. Mutable views deliberately do not expose unrestricted access to their backing
slice.

Builders use `new`, one fluent method per caller-supplied field, and `build_into`.
They preflight all plans, extents, conversions, arithmetic, and capacity before writing, so
an error leaves the complete destination unchanged. Relative range sources derive payload
lengths; absolute sources derive physical exclusive payload ends, including prior fixed and
prefix widths, padding, alignment, and ranges. Derived sources have no builder input or
setter; shared sources use identical algebra and values. `buf_end` has no source. Success
returns the bounded mutable view and its disjoint mutable suffix.

Generated error variants retain field or physical-position context. Their exact surface
is visible on each generated layout because it depends on the declared codecs and layout
shape.

A post-write finalizer must return the target field's exact semantic integer type. This
layout is rejected because a `BeU16` target requires `u16`, not `u32`:

```rust,compile_fail
use wire_repr::wire_repr;

fn wrong_checksum_type(_: &[u8]) -> u32 {
    0
}

wire_repr! {
    /// A layout whose finalizer deliberately returns the wrong type.
    pub layout IncorrectFinalizer {
        /// The finalizer target.
        field checksum: BeU16 {
            finalize: wrong_checksum_type(bytes(buf_start..buf_start));
        }
    }
}

let mut output = [0; 2];
let _ = IncorrectFinalizerBuilder::new().build_into(&mut output);
```

## Layout kinds

Sequential layouts normally infer physical placement from declaration order:

```rust
# use wire_repr::wire_repr;
wire_repr! {
    pub layout Frame {
        field payload_length: BeU16;
        field payload: bytes(current_pos..current_pos + payload_length);
        field checksum: BeU32;
    }
}
```

Fields, padding, and alignment may instead all use explicit one-based `position` values
when physical order must differ from declaration and API order. Mixing implicit and
explicit sequential placement is rejected.

Absolute layouts use mandatory zero-based offsets and may contain represented gaps:

```rust
# use wire_repr::wire_repr;
wire_repr! {
    pub absolute layout Header {
        field magic: bytes(4) { offset: 0; }
        field version: BeU16 { offset: 8; }
    }
}
```

Absolute layouts are fixed-width. They do not infer offsets and do not support prefix
fields, byte ranges, padding, or alignment.

See [`wire_repr!`] for the declaration grammar and generated-name reference.

## Field codecs and framing

Built-in fixed codecs cover unsigned 8/16/24/32/64/128-bit integers and signed
8/16/32/64/128-bit integers in the applicable byte orders. `bytes(N)` exposes an opaque
borrowed span through [`Bytes`].

Custom fixed-width fields use `codec(path)` and implement [`FixedCodec`]. Variable-width
prefix fields use `prefix(path)` and implement [`PrefixCodec`]. Both contracts separate
fallible planning from infallible emission through [`EncodePlan`]. This separation is what
lets generated setters and builders preserve whole-destination atomicity.

Sequential byte ranges borrow opaque spans in three forms:

- `bytes(current_pos..current_pos + source)` uses `source` as a relative length;
- `bytes(current_pos..source)` uses `source` as an exclusive absolute endpoint from
  representation byte zero;
- `bytes(current_pos..buf_end)` consumes the supplied view-buffer tail.

The first two require a physically preceding built-in fixed integer or semantic mapping over
one. Geometry uses the raw physical integer and checked `usize` conversion; prefix,
custom/direct, declared scalar, nominal, and byte-range sources are unsupported. `bytes(0)`
is invalid, although dynamic ranges may be empty. An absolute range does not itself end
`parse_prefix`: the suffix follows the complete represented layout, including later physical
fields. `buf_end` has no source, is physically terminal, and therefore produces an empty
suffix. Variable-width-source framing such as ULEB128 section lengths remains
consumer-owned.

The [`codec`] module documents the implementor laws and extension boundary.

### Total semantic mappings

An eligible built-in fixed integer or `bytes(N)` field may add `as TypePath`, for example
`field kind: BeU16 as crate::Kind;` or
`field address: bytes(4) as crate::Address;`. The physical codec still owns byte decoding,
encoding, range errors, and atomicity; the mapped type is the nominal consumer-facing API.

Mapped fields generate `field()` for the semantic value and `field_raw()` for the raw codec
value. Eligible mutable fields also generate semantic and `_raw` setters, and builders
generate both fluent forms for the same input slot: the last call wins. A byte-range
source exposes both getters but has neither setter nor builder input because the builder
derives its raw value from the byte range.

Mappings are deliberately total: getters use `Semantic: From<Raw>` and setters/builders
use `Raw: From<Semantic>`. Raw types are exact codec types, including `u32` for `U24` and
`[u8; N]` for `bytes(N)`. There is no fallible semantic conversion layer. Consequently a
semantic `U24` value can still produce an out-of-range raw `u32`, which the physical codec
rejects without changing the destination. Mapped byte values are owned arrays or wrappers;
unmapped `bytes(N)` continues to return borrowed [`Bytes`] (`&[u8]`).

Only built-in fixed integers and `bytes(N)` are eligible. Declared `scalar Name: Codec;`
instead declares a reusable codec-owning nominal wrapper; it is not an `as Type` mapping,
and mappings do not apply to declared scalar codecs, custom/direct codecs, prefix fields,
or byte ranges. A mapping compiles to direct `From` calls around the existing codec operations:
no runtime metadata, allocation, or dynamic dispatch is introduced.

## Deliberate limits

`wire-repr` is a byte-representation compiler, not a runtime schema system. It does not
provide reflection, descriptors, schema walkers, repeated sequences, tagged unions,
arbitrary conditional fields, checksums, protocol state, allocation policy, or I/O.
Custom codecs and consumer validation remain ordinary explicit Rust.
