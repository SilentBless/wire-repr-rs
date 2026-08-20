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

The layout owns physical facts such as widths, offsets, framing, and byte ranges.
Your application still owns protocol meaning such as magic values, reserved bits,
checksums, and cross-field policy.

> [!IMPORTANT]
> Generated views borrow ordinary byte slices. They do not reinterpret bytes as Rust
> structs, depend on alignment or ABI layout, allocate, use `unsafe`, or carry a
> runtime descriptor.

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

## ✍️ Edit existing bytes

Mutable views are separate because they borrow the input exclusively. They may change
same-width fields but cannot resize or invalidate the representation.

```rust
let mut bytes = GENESIS_HEADER;
let mut header = BitcoinBlockHeaderViewMut::parse_exact_mut(&mut bytes)
    .expect("valid block header");

header.set_nonce(0).expect("nonce has a fixed encoding");
assert_eq!(header.nonce(), 0);
```

Dynamic range sources, self-delimiting fields, and variable byte ranges do not receive
setters that could change framing. A mutable range accessor exposes only its already
validated span.

## 🏗️ Write a new representation

Builders use caller-owned output. They finish all fallible planning, derivation,
geometry, arithmetic, and capacity checks before changing the first byte.

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

If a builder returns an error, the complete supplied output slice remains unchanged.
On success, the builder writes only bytes owned by the representation. Any suffix,
absolute-layout gap, padding, or alignment span keeps its previous contents unless a
field explicitly covers it.

## 📏 Length-delimited payloads

Bitcoin P2P messages combine a fixed envelope with a payload whose length is stored in
the header. The readable DSL describes the relationship directly:

```rust
wire_repr! {
    pub layout BitcoinMessage {
        magic: LeU32;
        command: bytes(12);
        payload_length: LeU32;
        checksum: bytes(4);
        payload: bytes(payload_length);
    }
}
```

`payload_length` is physical geometry. Parsing reads it and validates the payload end.
Building derives it from the supplied payload, so callers cannot provide a conflicting
length.

```rust
let command = *b"block\0\0\0\0\0\0\0";
let checksum = [0x11, 0x22, 0x33, 0x44];
let payload = [1, 2, 3, 4];
let mut output = [0u8; 28];

let message = BitcoinMessageBuilder::new()
    .magic(0xd9b4_bef9)
    .command(&command)
    .checksum(&checksum)
    .payload(&payload)
    .build_into(&mut output)
    .expect("complete Bitcoin message")
    .0;

assert_eq!(message.payload_length(), 4);
assert_eq!(message.payload(), &payload);
```

Sequential layouts support three range forms:

| DSL | Meaning |
| --- | --- |
| `bytes(length)` | `length` bytes from the current position |
| `bytes_to(end)` | bytes up to an exclusive endpoint from representation byte zero |
| `remaining_bytes` | the bounded input tail |

A source must be an eligible physically preceding fixed integer, optionally exposed
through a total mapping. Dynamic ranges may be empty. `remaining_bytes` must be
physically last and consumes the complete supplied view buffer, so it cannot discover
an external packet boundary for you.

A full Bitcoin block contains a CompactSize transaction count followed by repeated,
self-delimiting transactions. Repeated sequences are intentionally outside the current
layout model; consumer code owns that cursor and parses each bounded item separately.
The block header itself is a natural fixed layout, while the P2P envelope is a natural
length-delimited layout.

## 🧭 Layout vocabulary

The common sequential form reads in physical order:

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

Use `@ N` only when API declaration order must differ from sequential wire order:

```rust
wire_repr! {
    pub layout Reordered {
        checksum @ 2: BeU16;
        tag      @ 1: U8;
    }
}
```

When explicit positions are used, every physical entry needs a unique contiguous
one-based number. Declaration order controls getters, builder inputs, documentation,
and declaration-oriented errors. Physical order controls parsing and writing.

Absolute layouts use zero-based byte offsets and make gaps explicit:

```rust
wire_repr! {
    pub absolute layout DatabasePageHeader {
        magic   @ 0: bytes(16);
        version @ 16: BeU32;
        page_id @ 24: BeU64;
    }
}
```

Their represented width is the largest field extent. Gaps are represented bytes and
builders preserve them. Absolute layouts are fixed-width only; they do not accept
padding, alignment, self-delimiting fields, or dynamic ranges.

Other field forms include:

```rust
fixed: crate::FixedCodec;
variable: variable(crate::SelfDelimitedCodec);
kind: BeU16 as crate::Kind;
tail: remaining_bytes;
```

Unsigned built-in integer fields may expose immutable bit projections:

```rust
flags: U8 {
    projections {
        bit enabled: 0;
        bits mode: 1..=3;
    }
};
```

Bit zero is the least-significant bit of the decoded integer regardless of wire
endianness. Projections add direct shifts and masks; they own no bytes or runtime
metadata.

For the full grammar, mappings, builder contexts, derived fields, finalizers, and
compile-time diagnostics, see the [`wire_repr!` reference](wire-repr-macros/src/wire_repr.md).

## 🔬 What reaches the CPU

Generated operations are direct safe Rust: bounded slice access, endian conversion,
shifts, masks, and copies. There are no schema walks, erased codecs, allocation, or
dynamic dispatch.

The release probes live in [`wire-repr/tests/codegen.rs`](wire-repr/tests/codegen.rs)
beside equivalent handwritten Rust. These x86-64 snippets were captured with Rust
1.91.0 (`f8297e351`), LLVM 21.1.2, `--release`, targeting
`x86_64-unknown-linux-gnu`. Compiler-local labels were shortened; instructions were
not changed.

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

**Guaranteed**

- Safe Rust, `no_std`, and no allocation.
- Borrowed views over exact represented bytes.
- Explicit complete-buffer or leading-representation framing.
- Validated dynamic boundaries retained by the view.
- Same-width mutation that cannot silently reframe bytes.
- All fallible builder work completed before output mutation.
- Direct generated code checked against handwritten release probes.

**Intentionally outside the crate**

- Domain and protocol validation.
- Repeated sequences and arbitrary conditional fields.
- Nested runtime schemas or reflection.
- I/O, buffering, transport state, and allocation policy.
- Cryptography and application-owned checksum policy.

The normative ownership and safety rules are in
[`ARCHITECTURE.md`](ARCHITECTURE.md). The published API reference is available on
[docs.rs](https://docs.rs/wire-repr).

## 📦 Workspace

- [`wire-repr`](wire-repr/) is the public runtime facade and macro re-export.
- [`wire-repr-macros`](wire-repr-macros/) is the host-side schema compiler.

Both packages are version `0.5.0`, use edition 2024, and support Rust 1.91. The target
runtime has empty default features, no dependencies, and `unsafe_code = "deny"`.

## 📄 License

MIT © 2026 SilentBless. See [LICENSE](LICENSE).
