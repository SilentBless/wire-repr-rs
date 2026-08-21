# Byte-backed representations without a runtime schema

`wire-repr` 0.5 compiles binary layout declarations into safe borrowed immutable
views, restricted mutable views, and builders over ordinary byte slices. It is safe
Rust, `no_std`, `no_alloc`, has empty default features, and targets Rust 1.91 / edition
2024. It does not use `unsafe`, allocation, alignment or ABI reinterpretation, a
runtime schema, or I/O.

A layout owns physical representation — bytes, widths, offsets, framing, and dynamic
boundaries. Consumer code owns protocol semantics: magic values, reserved-byte policy,
checksums, and cross-field validation.

## What a layout can express

- Fixed sequential layouts, dynamic sequential layouts, and fixed absolute-offset
  layouts; sequential declaration order may be separated from physical order with
  one-based placements.
- Fixed codecs and opaque `bytes(N)` fields; `padding(N)` and `align(N)`; absolute
  offsets and represented gaps.
- Validated `bytes(source)`, `bytes_to(source)`, and terminal `remaining_bytes` ranges.
  Immutable views retain their validated endpoints; mutable range access is bounded to
  that span; builders derive range sources from copied inputs or explicitly retain an
  existing destination span without rewriting it. An unannotated eligible source uses
  checked integer conversion; `range_source: Adapter` uses [`RangeSource`] to convert
  a direct built-in integer source and its byte geometry bidirectionally.
- Self-delimiting `variable(PrefixCodec)` fields with both decoded and exact accepted
  `_raw()` bytes. A variable field is not a dynamic-range source.
- Direct custom `FixedCodec` paths, top-level nominal scalar codecs, total `as`
  mappings with semantic/raw accessors, and unsigned decoded-integer LSB0 projections.
- Builder-only borrowed contexts, explicit fallible `derive` / `derive_error`, and
  infallible consumer-supplied finalizers over represented bytes, values, and context.
- Generic tagged choices with one unsigned fixed-integer tag, static or runtime
  discriminant selection, bodyless cases, and explicit unknown-body framing.

`Layout::view(bytes)` is a framing request. Use exactly one terminal operation:
`without_trailing()` validates that all input is one representation, while
`with_remainder()` returns one validated representation plus its suffix. `as_bytes()`
always excludes that suffix.

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

let bytes = [0u8; BitcoinBlockHeader::WIDTH];
let header = BitcoinBlockHeader::view(&bytes).without_trailing()?;
assert_eq!(header.version(), 0);
# Ok::<(), BitcoinBlockHeaderError>(())
```

For `pub layout Packet`, [`wire_repr!`] generates `Packet<'wire>` (the immutable
owner), `PacketViewMut<'wire>`, `PacketBuilder<'value>`, and structural parse,
mutation, and write errors. Fixed layouts also expose `Packet::WIDTH`. Mutable setters
exist only where a same-width write cannot reframe later bytes. Builders finish all
fallible planning, derivation, geometry, arithmetic, and capacity checks before the
first write, so any builder error leaves the complete supplied output unchanged.

Compile-time contracts reject incompatible generated operations. For example, a
finalizer must return its target field's exact semantic integer type:

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

Here `BeU16` requires `u16`, not `u32`.

## Real formats and extension points

The README uses the Bitcoin genesis header and links complete executable fixtures for
[PNG dynamic chunks](https://github.com/SilentBless/wire-repr-rs/blob/master/wire-repr/tests/consumer_formats/png.rs),
[SQLite absolute headers](https://github.com/SilentBless/wire-repr-rs/blob/master/wire-repr/tests/consumer_formats/sqlite.rs),
and [Wasm ULEB128 prefixes](https://github.com/SilentBless/wire-repr-rs/blob/master/wire-repr/tests/consumer_formats/wasm.rs).
It also links focused coverage for
[dynamic builders](https://github.com/SilentBless/wire-repr-rs/blob/master/wire-repr/tests/dynamic_sequential_builders.rs),
[prefixes](https://github.com/SilentBless/wire-repr-rs/blob/master/wire-repr/tests/prefix_sequential.rs),
[mappings](https://github.com/SilentBless/wire-repr-rs/blob/master/wire-repr/tests/mappings.rs),
[scalars](https://github.com/SilentBless/wire-repr-rs/blob/master/wire-repr/tests/scalars.rs),
and [bit projections](https://github.com/SilentBless/wire-repr-rs/blob/master/wire-repr/tests/bit_projections.rs).

Implement [`FixedCodec`] for a compile-time-width field or [`PrefixCodec`] for a
self-delimiting field; both use [`EncodePlan`] to separate fallible planning from
infallible writing. Invalid field combinations, placements, range sources, projection
forms, and derive/finalize contracts are rejected at compile time. See the
[`wire_repr!`] macro reference for the complete grammar and generated names, and the
[README](https://github.com/SilentBless/wire-repr-rs#readme) for compact real-format
examples.

## Tagged choices

A `choice` is a physical tagged representation, not a domain-policy switch. It owns one
unsigned built-in fixed-integer tag and one selected body. Static cases use literals;
case bodies name layouts declared in the same `wire_repr!` invocation, while bodyless
cases represent only their tag:

```rust
use wire_repr::wire_repr;

wire_repr! {
    pub layout PingBody { value: BeU16; }
    pub choice Message {
        tagged by kind: U8;
        Ping = 1: PingBody;
        Halt = 2;
    }
}

let message = Message::view(&[2]).without_trailing()?;
assert_eq!(message.kind(), 2);
assert_eq!(message.case(), MessageCase::Halt);
# Ok::<(), MessageError>(())
```

Dynamic choices declare `context tags: Type;` before `tagged by kind: U8 using tags;`.
Their cases omit literals and `Type` implements [`Discriminant`]<Raw, MessageCase>.
Its `resolve` returns `Ok(Some(case))`, `Ok(None)` for an unrecognized raw tag, or
`Err(error)`; `encode` selects the output tag during builder preparation and may
canonicalize it. Supply the context to the view request for parsing and to the builder
for preparation.

Unknown tags reject by default. The generated request accepts an explicit
[`UnknownBody`] policy: `Exact(n)` retains exactly that body length and leaves a suffix;
`Remainder` retains the supplied remainder. Choices never infer unknown framing or
resynchronize. Accepted unknown views retain raw tag and exact body bytes in the
generated `MessageUnknown`, allowing consumer code to explicitly rebuild a forwarded
unknown value.

Choice builders take already prepared selected body plans (for example,
`MessageBuilder::ping(PingBodyBuilder::new().value(7).prepare()?)`). Their `prepare`
performs discriminant resolution/encoding, tag planning, and length checks before commit;
commit prechecks capacity, and a short commit is atomic. Choices do not provide mutable
variant switching.

`wire-repr` deliberately excludes repeated sequences, arbitrary conditional fields or
predicates, multi-field discriminants, mutable variant switching, inferred unknown
framing, nested runtime schemas, reflection, I/O, and domain validation. A Bitcoin
CompactSize or Wasm LEB128 value can be a prefix field, but consumer code still owns the
cursor and boundaries for repeated items.
