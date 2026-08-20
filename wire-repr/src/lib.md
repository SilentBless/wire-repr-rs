# Byte-backed representations without a runtime schema

The release is version 0.5.0. `wire-repr` compiles a layout declaration into borrowed
immutable views, restricted mutable views, and builders for binary data. It works on
ordinary byte slices: no allocation, `unsafe`, alignment requirement, ABI
reinterpretation, runtime schema, or I/O.

The crate is safe Rust, `no_std`, `no_alloc`, has empty default features, and targets
Rust 1.91 / edition 2024. A layout owns physical representation — bytes, widths,
offsets, and framing. Consumer code owns magic values, reserved-byte policy,
checksums as protocol policy, and cross-field semantics.

## Read and build

`Layout::view(bytes)` is only a framing request. Exactly one terminal parses and
validates the layout:

```rust
use wire_repr::wire_repr;

wire_repr! {
    pub layout BitcoinBlockHeader {
        version: LeI32;
        previous_block_hash: bytes(32);
        merkle_root: bytes(32);
        timestamp: LeU32;
        target_bits: LeU32;
        nonce: LeU32;
    }
}

let mut input = [0u8; 82];
input[80..].copy_from_slice(&[0xaa, 0xbb]);
let (header, remainder) = BitcoinBlockHeader::view(&input)
    .with_remainder()
    .expect("valid leading block header");
assert_eq!(header.as_bytes(), &input[..80]);
assert_eq!(remainder, &[0xaa, 0xbb]);
assert_eq!(header.version(), 0);

let hash = [0u8; 32];
let mut output = [0u8; BitcoinBlockHeader::WIDTH];
let (built, suffix) = BitcoinBlockHeaderBuilder::new()
    .version(1)
    .previous_block_hash(&hash)
    .merkle_root(&hash)
    .timestamp(1_231_006_505)
    .target_bits(0x1d00_ffff)
    .nonce(2_083_236_893)
    .build_into(&mut output)
    .expect("complete builder and sufficient output");
assert_eq!(built.version(), 1);
assert!(suffix.is_empty());
```

Use `without_trailing()` when all input must be the representation:

```rust
# use wire_repr::wire_repr;
# wire_repr! { pub layout OneByte { value: U8; } }
let bytes = [42];
let view = OneByte::view(&bytes).without_trailing()?;
assert_eq!(view.value(), 42);
# Ok::<(), OneByteError>(())
```

`with_remainder()` validates one complete representation and returns its suffix;
that suffix is excluded from `as_bytes()`. `without_trailing()` performs the same
single structural parse and rejects a nonempty suffix. Immutable dynamic views retain
the validated self-delimiting and range endpoints, so getters do not re-scan or
reframe bytes.

## Generated API

For `pub layout Packet`, [`wire_repr!`] generates:

| Item | Role |
| --- | --- |
| `Packet<'wire>` | Immutable borrowed representation, `view`, `as_bytes`, and getters |
| `PacketViewMut<'wire>` | Mutable representation, `parse_prefix_mut`, `parse_exact_mut`, and eligible setters |
| `PacketBuilder<'value>` | Fluent inputs and `build_into` |
| `PacketError` | Structural parsing and exact-framing errors |
| `PacketMutationError` | Setter planning and mutation errors |
| `PacketWriteError` | Builder preflight and capacity errors |

Fixed layouts also expose `Packet::WIDTH`. A `variable(path)` field exposes a decoded
getter and `name_raw()` containing the exact accepted bytes; a raw getter never
re-encodes the decoded value. Mapped fields expose semantic and raw forms. See
[`wire_repr!`] for declaration syntax and generated-name details.

## Layout families and mutation

Sequential layouts are physical entries in order, optionally using a complete,
contiguous set of one-based `@ N` placements to separate physical order from
public API order. They support fixed fields, self-delimiting codecs, opaque
`padding(N)`/`align(N)`, and validated dynamic byte ranges. Absolute layouts use a
mandatory zero-based `@ BYTE_OFFSET`, are fixed width, and preserve represented gaps;
they do not support padding, alignment, self-delimiting codecs, or ranges.

Mutable parsing deliberately remains `PacketViewMut::parse_prefix_mut` or
`parse_exact_mut`, not `Packet::view`. Setters exist only for same-width fixed fields
whose writes cannot invalidate framing. Self-delimiting codecs, ranges,
`remaining_bytes`, and range-source fields have no setter; a range instead has a
bounded mutable-slice accessor.

Builders have a hard preflight/commit boundary. Before changing any output byte, they
check inputs and contexts, codec plans and lengths, derivations, source conversions,
shared-source agreement, dynamic geometry, arithmetic, and capacity. Therefore every
builder error leaves the entire supplied output slice unchanged. On success, padding,
alignment, absolute-layout gaps, and the returned suffix retain their prior bytes
unless the representation explicitly writes them.

```rust,compile_fail
use wire_repr::wire_repr;

fn wrong_checksum_type(_: &[u8]) -> u32 { 0 }

wire_repr! {
    pub layout IncorrectFinalizer {
        checksum: BeU16 {
            finalize: wrong_checksum_type(bytes(buf_start..buf_start));
        };
    }
}

let mut output = [0; 2];
let _ = IncorrectFinalizerBuilder::new().build_into(&mut output);
```

A finalizer target must return that target's exact semantic integer type; here `BeU16`
requires `u16`, not `u32`.

## Codecs and limits

Built-in fixed codecs cover unsigned 8/16/24/32/64/128-bit and signed
8/16/32/64/128-bit values in applicable byte orders; `bytes(N)` is an opaque borrowed
span. Implement [`FixedCodec`] for a compile-time-width field or [`PrefixCodec`] for
self-delimiting structural discovery. Both use [`EncodePlan`] to separate fallible
planning from infallible writing. The [`codec`] module documents these extension contracts.

`wire-repr` intentionally excludes runtime schemas, reflection, repeated sequences,
tagged unions, arbitrary conditional fields, nested ranges, independently owned
bitfields, parser-owned protocol semantics, allocation policy, and target-runtime
dependencies. A `variable(path)` field is structural, not a dynamic-range source.
