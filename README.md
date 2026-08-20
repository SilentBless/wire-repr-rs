<h1 align="center">wire-repr</h1>

<p align="center">
  <strong>Compile binary layouts into safe borrowed views and atomic writers.</strong>
</p>

<p align="center">
  <code>no_std</code> · <code>no_alloc</code> · safe Rust · no runtime schema
</p>

`wire-repr` is for binary formats whose bytes are the source of truth: network
packets, file headers, storage pages, firmware records, and IPC messages. A compact
layout declaration becomes direct, specialized Rust for reading existing bytes,
editing fixed-width fields, and writing new representations.

The layout owns physical facts — widths, offsets, framing, and byte ranges. Consumer
code owns protocol meaning: magic values, reserved bits, checksums, and cross-field
policy.

> [!IMPORTANT]
> Generated views borrow ordinary byte slices. They do not reinterpret bytes as Rust
> structs, depend on alignment or ABI layout, allocate, use `unsafe`, or carry a
> runtime descriptor.

## 🗺️ Capability map

- **Three layout families:** fixed sequential, dynamic sequential, and fixed absolute
  layouts; physical ordering can differ from declaration order.
- **Framing and geometry:** exact or prefix parsing, `bytes(source)`,
  `bytes_to(source)`, `remaining_bytes`, and retained validated endpoints.
- **Typed bytes:** built-in codecs, direct `FixedCodec` paths, nominal scalar codecs,
  total `as` mappings, and unsigned LSB0 projections.
- **Variable encodings:** `variable(PrefixCodec)` preserves accepted raw prefix bytes
  while exposing a decoded value.
- **Safe change paths:** immutable views, framing-safe mutable views, and builders
  with all-or-nothing output commits.
- **Computed writers:** derived fields, borrowed builder context, and infallible
  post-write finalizers over exact represented spans and values.
- **Extension points:** `FixedCodec`, `PrefixCodec`, and `EncodePlan`, with malformed
  layout declarations rejected at compile time.

Complete executable format fixtures: [PNG chunks](wire-repr/tests/consumer_formats/png.rs),
[SQLite headers](wire-repr/tests/consumer_formats/sqlite.rs), and
[Wasm ULEB128](wire-repr/tests/consumer_formats/wasm.rs).

## 📦 Installation

```toml
[dependencies]
wire-repr = { version = "0.5", default-features = false }
```

Rust 1.91 is the minimum supported version. The crate has no default features and no
target-runtime dependencies.

## 🚀 Start with a real format

A Bitcoin block starts with an 80-byte block header. The wire representation mixes
little-endian integers with two opaque 32-byte hashes:

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
```

The layout name is also the immutable borrowed type. `view` creates a lightweight
request; the terminal operation performs structural validation exactly once.

```rust
const GENESIS_HEADER: [u8; 80] = [
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x3b, 0xa3, 0xed, 0xfd,
    0x7a, 0x7b, 0x12, 0xb2, 0x7a, 0xc7, 0x2c, 0x3e,
    0x67, 0x76, 0x8f, 0x61, 0x7f, 0xc8, 0x1b, 0xc3,
    0x88, 0x8a, 0x51, 0x32, 0x3a, 0x9f, 0xb8, 0xaa,
    0x4b, 0x1e, 0x5e, 0x4a, 0x29, 0xab, 0x5f, 0x49,
    0xff, 0xff, 0x00, 0x1d, 0x1d, 0xac, 0x2b, 0x7c,
];

let header = BitcoinBlockHeader::view(&GENESIS_HEADER)
    .without_trailing()
    .expect("valid block header");

assert_eq!(header.version(), 1);
assert_eq!(header.timestamp(), 1_231_006_505);
assert_eq!(header.target_bits(), 0x1d00_ffff);
assert_eq!(header.nonce(), 2_083_236_893);
assert_eq!(header.previous_block_hash(), &[0; 32]);
assert_eq!(header.as_bytes(), &GENESIS_HEADER);
```

Use `with_remainder` when the input continues after one representation:

```rust
let mut framed = [0u8; 82];
framed[..80].copy_from_slice(&GENESIS_HEADER);
framed[80..].copy_from_slice(&[0xaa, 0xbb]);

let (header, remainder) = BitcoinBlockHeader::view(&framed)
    .with_remainder()
    .expect("valid leading block header");

assert_eq!(header.as_bytes(), &framed[..80]);
assert_eq!(remainder, &[0xaa, 0xbb]);
```

`without_trailing` validates the same representation but rejects any suffix.
`as_bytes` always returns exactly the represented bytes, never the remainder.

## 📐 Layouts, ordering, and physical holes

A fixed sequential layout reads entries in physical order. `padding(N)` and `align(N)`
are represented opaque spans, not fields with invented values:

```rust
wire_repr! {
    pub layout Record {
        tag: U8;
        padding(3);
        align(8);
        value: BeU16;
    }
}
```

One-based placements let declaration/API order differ from wire order; parsing and
writing still follow physical order:

```rust
wire_repr! {
    pub layout Reordered {
        checksum @ 2: BeU16;
        tag      @ 1: U8;
    }
}
```

An absolute layout instead uses zero-based byte offsets. It is fixed width; its gaps
are represented and builders preserve them. The
[SQLite 100-byte header fixture](wire-repr/tests/consumer_formats/sqlite.rs) is the
complete real-format example, including offsets, a preserved 20-byte reserved span,
mutable field writes, exact framing, and consumer-owned SQLite validation.

```rust
wire_repr! {
    pub absolute layout DatabasePageHeader {
        magic   @ 0: bytes(16);
        version @ 16: BeU32;
        page_id @ 24: BeU64;
    }
}
```

Absolute layouts do not accept padding, alignment, self-delimiting fields, or dynamic
ranges. Sequential layouts come in fixed and dynamic forms.

## 📏 Dynamic geometry and framing

The PNG chunk is the ordinary relative case: its `data_length` physically precedes
`data`, and structural parsing validates and retains the resulting endpoint.

```rust
wire_repr! {
    pub layout PngChunk {
        data_length @ 1: BeU32;
        chunk_type  @ 2: bytes(4);
        data        @ 3: bytes(data_length);
        crc         @ 4: BeU32;
    }
}
```

[`png.rs`](wire-repr/tests/consumer_formats/png.rs) parses IHDR/IEND, retains exact
ranges, and derives `data_length` from the builder's `data` input. Bounded range
mutation is exercised directly in
[`dynamic_sequential_writes.rs`](wire-repr/tests/dynamic_sequential_writes.rs). PNG CRC
checking remains consumer validation, not structural parsing.

The other range forms are equally literal:

```rust
wire_repr! {
    pub layout IndexedRecord {
        end: BeU16;
        body: bytes_to(end); // exclusive endpoint from representation byte zero
    }

    pub layout TerminalRecord {
        header: U8;
        body: remaining_bytes; // physically last; consumes this supplied view buffer
    }
}
```

`bytes(source)` takes that many bytes from the current position; `bytes_to(source)`
uses an exclusive endpoint from representation byte zero; `remaining_bytes` has no
external framing magic and cannot discover a packet boundary for you. Eligible sources
are physically preceding fixed integers (including a total-mapped integer). Dynamic
ranges may be empty. The view retains validated endpoints so getters do not re-scan or
reframe the bytes.

## 🧩 Typed fields without losing raw bytes

Use a direct `FixedCodec` path when a field already has one, and a top-level scalar
when a protocol needs a nominal fixed-width type. Total `as` mappings expose semantic
and raw getters. Unsigned projections use decoded-integer LSB0 numbering regardless of
wire byte order:

```rust
wire_repr! {
    pub scalar ProtocolVersion: LeI32;

    pub layout BitcoinVersionServices {
        version: ProtocolVersion;
        services: LeU64 as crate::Services {
            projections {
                bit network: 0;
                bit witness: 3;
            }
        };
    }
}
```

That declaration yields the declared semantic getters plus `services_raw()`; it does
not invent domain validation. See executable
[scalar/direct-codec coverage](wire-repr/tests/scalars.rs),
[mapping coverage](wire-repr/tests/mappings.rs), and
[projection coverage](wire-repr/tests/bit_projections.rs).

## 🔖 Self-delimiting prefixes are fields, not range sources

`variable(path)` uses a `PrefixCodec` to discover one structural field. A Wasm `u32`
ULEB128 is a natural example:

```rust
wire_repr! {
    pub layout WasmImmediate {
        opcode: U8;
        index: variable(crate::U32Leb128);
    }
}
```

The generated `index()` decodes the value and `index_raw()` returns the exact accepted
wire bytes. Legal noncanonical input remains preserved by `_raw()` — for example, ULEB128
`[0x85, 0x00]` still means `5` — while a builder plan may write the codec's canonical
encoding. The complete codec is in [the Wasm fixture](wire-repr/tests/consumer_formats/wasm.rs);
[prefix layout tests](wire-repr/tests/prefix_sequential.rs) exercise raw spans,
multiple prefixes, errors, and borrowed decoded values.

Bitcoin CompactSize is another honest prefix-codec use for one count/value. It is **not**
a dynamic-range source, and a repeated transaction sequence remains consumer-owned:
keep the cursor, bound each item, and parse it separately. Tagged unions, arbitrary
conditional/version-selected fields, repeated sequences, and nested schemas are not
layout features in 0.5.

Versioned formats use the same ownership boundary: parse a stable prefix containing the
version, then let consumer code select a separate nominal `V1Body` or `V2Body` layout for
the bounded remainder. The macro does not hide that dispatch inside a generated union or
silently merge version-specific policy into structural parsing.

## ✍️ Mutable views and builders

Mutable views borrow exclusively and can change only same-width fixed fields. They do
not offer setters for range sources, dynamic ranges, `remaining_bytes`, or
self-delimiting prefixes — changing those could reframe later bytes. A dynamic range has
a bounded mutable-slice accessor over its validated span.

```rust
let mut chunk_bytes = [
    0, 0, 0, 2, b't', b'E', b'S', b'T', b'a', b'b', 0, 0, 0, 0,
];
let mut chunk = PngChunkViewMut::parse_exact_mut(&mut chunk_bytes)
    .expect("structurally valid PNG chunk");
chunk.data_mut().copy_from_slice(b"XY");
assert_eq!(chunk.data(), b"XY");
```

Builders use caller-owned output. They derive range sources implicitly, plan custom
codecs, perform fallible derivations and all geometry/capacity checks, then commit.
Every builder error leaves the **entire supplied output slice** unchanged; successful
writes preserve suffixes, padding, alignment spans, absolute gaps, and existing ranges
unless an explicit field covers them.

For a dynamic range that is already populated in the destination, the generated
`body_existing(length)` form retains that span instead of copying new bytes. Its length
still participates in geometry, derivation, and finalizer spans, while the builder never
rewrites the retained range.

The following compact fixture syntax shows explicit fallible derivation, borrowed
builder-only context, and an infallible finalization policy:

```rust
wire_repr! {
    pub layout DerivedAssembly {
        tag: U8;
        options_length: U8;
        options: bytes(options_length);
        payload_length: U8;
        payload: bytes(payload_length);
        total: U8 {
            derive: crate::derive_total(value(options_length), len(payload));
            derive_error: crate::DeriveFailure;
        };
    }

    pub layout ContextFinalization {
        context seed: [u8];
        tag: U8;
        checksum: BeU16 {
            finalize: crate::finalize_context_checksum(
                bytes(checksum.start..checksum.end),
                context(seed),
            );
        };
    }
}
```

A finalizer runs after the ordinary commit inputs and is an infallible, consumer-supplied
policy. This fixture reads the zeroed target span; a real Bitcoin checksum would pass the
payload span and use consumer-supplied crypto. The structural parser does not validate
it. Finalizers can consume byte spans, semantic values, and borrowed context, and their
dependencies are resolved before calls. Complete derivation,
existing-range, finalizer, ordering, and atomicity cases are in
[`dynamic_sequential_builders.rs`](wire-repr/tests/dynamic_sequential_builders.rs).

A fixed Bitcoin header builder stays pleasantly boring:

```rust
let zero_hash = [0u8; 32];
let merkle_root = [0x42u8; 32];
let mut output = [0u8; BitcoinBlockHeader::WIDTH];

let (header, remainder) = BitcoinBlockHeaderBuilder::new()
    .version(1)
    .previous_block_hash(&zero_hash)
    .merkle_root(&merkle_root)
    .timestamp(1_700_000_000)
    .target_bits(0x1d00_ffff)
    .nonce(7)
    .build_into(&mut output)
    .expect("complete builder and sufficient output");

assert_eq!(header.nonce(), 7);
assert!(remainder.is_empty());
```

## 🔧 Custom codecs and diagnostics

Implement `FixedCodec` for a compile-time-width field, or `PrefixCodec` for a
self-delimiting field. Both produce an `EncodePlan`: planning may fail, while
`write_into` is the infallible commit step. The compiler also rejects invalid physical
placements, unsupported layout/form combinations, unsuitable dynamic sources, invalid
projection declarations, and incompatible derive/finalize contracts at compile time.

Read the [codec contracts](wire-repr/src/codec/mod.md) and the complete
[`wire_repr!` reference](wire-repr-macros/src/wire_repr.md) for grammar and diagnostic
surface. The linked executable fixtures above are the runnable behavior, not decorative
pseudocode.

## 🧬 Generated API and cost model

For `pub layout Packet`, `wire_repr!` generates `Packet<'wire>` (immutable view),
`PacketViewMut<'wire>` (restricted mutable view), `PacketBuilder<'value>`, and
structural parse/mutation/write error types. The immutable owner is the layout stem;
fixed layouts additionally expose `Packet::WIDTH`.

Generated operations are direct safe Rust: bounded slice access, endian conversion,
shifts, masks, and copies. There are no schema walks, erased codecs, allocation, or
dynamic dispatch. Release probes in [wire-repr/tests/codegen.rs](wire-repr/tests/codegen.rs)
compare these paths with handwritten safe Rust.

The following x86-64 snippets were captured with Rust 1.91.0 (`f8297e351`), LLVM
21.1.2, `--release`, targeting `x86_64-unknown-linux-gnu`. Compiler-local labels were
shortened; instructions were not changed.

<details>
<summary><strong>Fixed big-endian getter</strong></summary>

```asm
cmpq    $2, %rsi
jne     .invalid
movzwl  (%rdi), %edx
rolw    $8, %dx
movw    $1, %ax
retq
.invalid:
xorl    %eax, %eax
retq
```

</details>

<details>
<summary><strong>Bit projection</strong></summary>

```asm
movb    $2, %al
cmpq    $2, %rsi
jne     .done
movzbl  1(%rdi), %eax
andb    $1, %al
.done:
retq
```

</details>

<details>
<summary><strong>Same-width mutation</strong></summary>

```asm
cmpq    $2, %rsi
jne     .done
rolw    $8, %dx
movw    %dx, (%rdi)
.done:
cmpq    $2, %rsi
sete    %al
retq
```

</details>

<details>
<summary><strong>Fixed builder</strong></summary>

```asm
cmpq    $2, %rsi
jb      .done
rolw    $8, %dx
movw    %dx, (%rdi)
.done:
cmpq    $2, %rsi
setae   %al
retq
```

</details>

<details>
<summary><strong>Relative-range getter</strong></summary>

```asm
xorl    %eax, %eax
testq   %rsi, %rsi
je      .done
cmpq    $1, %rsi
je      .done
decq    %rsi
movzbl  (%rdi), %ecx
cmpq    %rcx, %rsi
jne     .done
movzbl  1(%rdi), %edx
movb    $1, %al
.done:
retq
```

</details>

Assembly snapshots are compiler-, target-, and probe-specific. The normative gate is
[`ci/check-codegen.py`](ci/check-codegen.py), which compares generated operations with
handwritten safe Rust and rejects unwanted calls, panic paths, allocation, dynamic
dispatch, and excess instruction shape.

## ✅ Contract and limits

**Guaranteed:** safe Rust, `no_std`, no allocation, borrowed exact represented bytes,
explicit exact/prefix framing, retained validated dynamic boundaries, framing-safe
mutation, and builder preflight before commit.

**Intentionally outside the crate:** domain and protocol validation; repeated sequences;
tagged unions and arbitrary conditional fields; nested runtime schemas or reflection;
I/O, buffering, transport state, and allocation policy; cryptography and checksum
policy.

The normative ownership and safety rules are in [ARCHITECTURE.md](ARCHITECTURE.md).
The published API reference is on [docs.rs](https://docs.rs/wire-repr).

## 📦 Workspace

- [`wire-repr`](wire-repr/) is the public runtime facade and macro re-export.
- [`wire-repr-macros`](wire-repr-macros/) is the host-side schema compiler.

Both packages are version `0.5.0`, use edition 2024, and support Rust 1.91. The target
runtime has empty default features, no dependencies, and `unsafe_code = "deny"`.

## 📄 License

MIT © 2026 SilentBless. See [LICENSE](LICENSE).
