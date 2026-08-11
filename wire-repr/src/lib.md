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
extent while validating prefix fields, bounded regions, padding, and alignment.

## Generated API

For `layout Packet`, [`wire_repr!`] generates a family centered on the declaration:

| Generated item | Purpose |
| --- | --- |
| `PacketView<'wire>` | Immutable borrowed representation and field getters |
| `PacketViewMut<'wire>` | Constrained mutable representation and eligible setters |
| `PacketBuilder` | Fluent caller-buffer construction |
| `PacketError` | Structural parse errors |
| `PacketMutationError` | Fixed-field planning and setter errors |
| `PacketWriteError` | Missing inputs, planning, extent, and capacity errors |

A field getter has the declared field name. Prefix fields also expose
`<field>_encoded()` so callers can inspect their exact accepted encoding. Bit projections
produce named `bool` or unsigned scalar getters without becoming independent byte owners.

Mutable views provide `parse_prefix_mut`, `parse_exact_mut`, `as_view`, and `into_view`.
Setters are generated only for same-width fields whose mutation cannot invalidate dynamic
framing. Mutable views deliberately do not expose unrestricted access to their backing
slice.

Builders use `new`, one fluent method per caller-supplied field, and `build_into`.
Length fields used by `region(source)` are derived from region inputs rather than supplied
separately. Builders finish all fallible planning and capacity checks before the first
write, so an error leaves the complete destination unchanged. Success returns the bounded
mutable view and its disjoint mutable suffix.

Generated error variants retain field or physical-position context. Their exact surface
is visible on each generated layout because it depends on the declared codecs and layout
shape.

## Layout kinds

Sequential layouts normally infer physical placement from declaration order:

```rust
# use wire_repr::wire_repr;
wire_repr! {
    pub layout Frame {
        field payload_length: BeU16;
        field payload: region(payload_length);
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
fields, regions, padding, or alignment.

See [`wire_repr!`] for the declaration grammar and generated-name reference.

## Field codecs and framing

Built-in fixed codecs cover unsigned 8/16/24/32/64/128-bit integers and signed
8/16/32/64/128-bit integers in the applicable byte orders. `bytes(N)` exposes an opaque
borrowed span through [`Bytes`].

Custom fixed-width fields use `codec(path)` and implement [`FixedCodec`]. Variable-width
prefix fields use `prefix(path)` and implement [`PrefixCodec`]. Both contracts separate
fallible planning from infallible emission through [`EncodePlan`]. This separation is what
lets generated setters and builders preserve whole-destination atomicity.

`region(source)` borrows an opaque span whose length is decoded from an earlier physical
field. Consumers can pass that span to a protocol-specific parser without teaching the
layout compiler domain semantics.

The [`codec`] module documents the implementor laws and extension boundary.

## Deliberate limits

`wire-repr` is a byte-representation compiler, not a runtime schema system. It does not
provide reflection, descriptors, schema walkers, repeated sequences, tagged unions,
arbitrary conditional fields, checksums, protocol state, allocation policy, or I/O.
Custom codecs and consumer validation remain ordinary explicit Rust.
