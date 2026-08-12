<h1 align="center">wire-repr</h1>

<p align="center">
  <strong>Zero-cost byte-backed representations for binary formats.</strong>
</p>

`wire-repr` generates safe borrowed views, constrained mutable views, and atomic
caller-buffer builders from compact layout declarations. It is designed for network
protocols, file headers, storage pages, firmware formats, IPC, and other binary data
where exact bytes and explicit ownership matter.

> [!IMPORTANT]
> Generated views borrow ordinary byte slices. They do not reinterpret bytes as Rust
> structs and do not depend on alignment, ABI layout, allocation, or `unsafe`.

---

## ✨ What it does

- **Zero-copy views.** Parse directly over caller-owned bytes and retain exact represented
  spans, including legal noncanonical prefix encodings.
- **Direct generated code.** Fixed getters compile to ordinary loads, endian conversions,
  shifts, and masks—without runtime schemas, reflection, or field lookup.
- **Atomic writes.** Builders plan the complete representation before touching caller
  output; an error leaves the whole destination unchanged.
- **Explicit framing.** `parse_prefix` returns one bounded representation plus its suffix,
  while `parse_exact` rejects unrelated trailing bytes.
- **Consumer-owned semantics.** The framework owns bounds and layout. Consumers keep
  ownership of magic values, reserved-byte policy, checksums, and cross-field rules.
- **Small runtime.** The public crate is `no_std`, `no_alloc`, dependency-free at target
  runtime, and safe Rust only.

## 🚀 Quick start

Add the facade crate:

```toml
[dependencies]
wire-repr = { version = "0.2", default-features = false }
```

Declare a sequential layout. Physical placement is inferred from declaration order:

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
let (view, suffix) = HeaderView::parse_prefix(&input).expect("valid header");

assert_eq!(view.as_bytes(), &input[..4]);
assert_eq!(suffix, &[0xff]);
assert_eq!(view.kind(), 7);
assert_eq!(view.length(), 256);
assert!(view.enabled());
assert_eq!(view.mode(), 5);

let mut output = [0u8; 4];
let (built, suffix) = HeaderBuilder::new()
    .kind(7)
    .length(256)
    .flags(0b0000_1011)
    .build_into(&mut output)
    .expect("complete builder");

assert_eq!(built.as_bytes(), &input[..4]);
assert!(suffix.is_empty());
```

> [!NOTE]
> `parse_prefix` excludes the suffix from the generated view. Use `parse_exact` when the
> entire input must be exactly one representation.

## 🧭 Layout model

### Sequential layouts

Sequential layouts use source order by default. Fields, padding, and alignment occupy
contiguous one-based physical positions:

```rust
wire_repr::wire_repr! {
    pub layout Record {
        field kind: U8;
        padding { length: 3; }
        align { boundary: 8; }
        field flags: BeU16;
    }
}
```

Use explicit `position` on **every** physical entry only when wire order must differ from
API and documentation order:

```rust
wire_repr::wire_repr! {
    pub layout Reordered {
        field checksum: BeU16 { position: 2; }
        field tag: U8 { position: 1; }
    }
}
```

Mixing explicit and implicit placement is rejected. Declaration order always controls
the generated API and rustdoc order; explicit positions control only physical order.

### Absolute layouts

Absolute layouts use mandatory zero-based byte offsets:

```rust
wire_repr::wire_repr! {
    pub absolute layout DatabaseHeader {
        field magic: bytes(16) { offset: 0; }
        field version: BeU32 { offset: 16; }
    }
}
```

Gaps remain represented bytes and are preserved verbatim. Overlapping codec extents are
rejected before input access. Absolute layouts are fixed-width and deliberately do not
infer offsets or support padding and alignment entries.

## 🧩 Fields and framing

### Fixed values and byte spans

Built-in fixed codecs cover unsigned 8/16/24/32/64/128-bit integers and signed
8/16/32/64/128-bit integers in the applicable byte orders. Fixed codecs decode every exact-width bit pattern; domain
validation remains consumer-owned.

Use `bytes(N)` when a field owns fixed-width bytes without interpreting them. Its getter
returns the original borrowed `&[u8]`. Builders and setters check only the exact width
before mutation.

### Total semantic mappings

An eligible built-in fixed integer or `bytes(N)` field can expose a nominal
domain-facing type while retaining its physical wire codec:

```rust
wire_repr::wire_repr! {
    pub layout Message {
        field kind: BeU16 as crate::Kind;
        field address: bytes(4) as crate::Address;
    }
}
```

`as TypePath` comes immediately after the codec, before placement or projections. It is
not a codec declaration: `kind()` returns `Kind`, `kind_raw()` returns the codec's raw
`u16`, and the corresponding setters and builder methods accept either form. This requires
total `Kind: From<u16>` and `u16: From<Kind>` conversions; `bytes(4)` similarly maps
between its semantic type and `[u8; 4]`. The raw mapping is exact (`U24` is `u32`), with no
fallible conversion layer. Mapped byte values are owned arrays or wrappers; unmapped
`bytes(N)` remains borrowed `&[u8]`.

Declared `scalar Name: Codec;` has a different job: it creates a reusable nominal wrapper
that owns a codec. `as Type` maps one eligible built-in physical field through `From`; it
does not apply to declared scalar, custom/direct, prefix, or region fields.

### Bit projections

Unsigned built-in storage fields can expose named immutable projections:

```rust
field flags: U8 {
    projections {
        bit enabled: 0;
        bits mode: 1..=3;
    }
}
```

Bit zero is the decoded value's least-significant bit regardless of wire endianness.
The storage field remains the only byte owner; projection getters are direct shift/mask
operations with no runtime metadata or dispatch. On a mapped integer field, projections
still read the physical decoded raw integer, not the semantic wrapper.

### Prefix fields

A sequential field backed by a custom `PrefixCodec` discovers its exact encoded width
during structural parsing:

```rust
field name: prefix(crate::Terminated);
```

The generated view preserves the exact accepted bytes. `name()` returns the decoded
value, while `name_encoded()` exposes its original encoding. Parsing validates the
prefix extent once and rejects any codec claim beyond the remaining input.

### Bounded regions

A region borrows an opaque span whose length comes from an earlier physical field:

```rust
wire_repr::wire_repr! {
    pub layout Frame {
        field payload_length: BeU16;
        field payload: region(payload_length);
        field checksum: BeU32;
    }
}
```

Dynamic builders accept the region bytes and derive its length source automatically.
A source may be declared later in explicit-position source order, but it must physically
precede every region it frames. Regions may be empty and remain available as exact
borrowed bytes for a consumer-owned inner parser.

### Terminal remainder

A sequential layout may end with one opaque `remainder` field. It owns every byte left
in the caller-supplied input after its preceding entries; it is not length-framed and may
be empty. For example, an Ethernet-like envelope keeps its payload caller-bounded rather
than guessing a transport boundary:

```rust
wire_repr::wire_repr! {
    pub layout EthernetEnvelope {
        field destination: bytes(6);
        field source: bytes(6);
        field ether_type: BeU16;
        field payload: remainder;
    }
}

let input = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0x00, 0x11, 0x22, 0x33, 0x44, 0x55,
    0x08, 0x00, 0x45, 0x00,
];
let (frame, suffix) = EthernetEnvelopeView::parse_prefix(&input).expect("frame");

assert_eq!(frame.payload(), &[0x45, 0x00]);
assert!(suffix.is_empty());
```

Because the remainder consumes all caller-bounded input, `parse_prefix` returns an empty
suffix and `parse_exact` accepts the same input. It does not identify an external packet,
transport, or FCS boundary.

> [!TIP]
> Keep unsupported or application-specific material as `bytes(N)` or `region(length)`,
> then parse it with a small consumer-owned view. The framework should not learn domain
> policy merely to move a slice boundary.

## ✍️ Mutation and building

Generated mutable views preserve the same represented extent as immutable views.
Same-width fixed fields receive typed setters when changing them cannot invalidate
region framing. Prefix fields, regions, remainders, and region length sources do not
receive in-place setters; regions and remainders expose mutable slices of exactly their
validated spans, and mutable views never expose unrestricted access to the full backing
slice.

Builders preflight codec plans, checked arithmetic, derived region lengths, remainder
lengths, and output capacity before writing. A successful build returns the bounded
mutable view together with its disjoint suffix. A failed build leaves every caller-owned
output byte unchanged.
Padding, alignment bytes, absolute gaps, and suffixes are therefore preserved rather
than silently normalized.

## 🔬 What reaches the CPU

Generated fixed-layout operations are ordinary safe Rust: direct byte loads, endian
conversion, shifts, masks, and bounded copies. There are no runtime descriptors, schema
walkers, erased codecs, hidden allocation, or dynamic dispatch.

For example, with Rust 1.91.0 targeting `x86_64-unknown-linux-gnu`, the generated
big-endian `u16` getter and its handwritten safe-Rust equivalent compile to the same
optimized body (compiler-local labels simplified):

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

The generated fixed builder is likewise merged by the optimizer with its handwritten
equivalent. Its complete operation shape is a capacity check, endian conversion, and
one store—no framework calls:

```asm
cmpq    $2, %rsi
jb      .short
rolw    $8, %dx
movw    %dx, (%rdi)
.short:
cmpq    $2, %rsi
setae   %al
retq
```

The [probe source](wire-repr/tests/codegen.rs) covers getters, projections, mutation,
and builders. The [pinned release-codegen gate](ci/check-codegen.py) compares each one
against equivalent handwritten safe Rust and rejects extra instructions, calls, panic
paths, allocation, or dynamic dispatch:

```sh
python3 ci/check-codegen.py
```

The stable contract is the optimized operation shape and absence of framework
machinery—not fragile textual assembly snapshots tied to register allocation or labels.

## ⚠️ Deliberate limits

> [!NOTE]
> `wire-repr` is a byte-representation compiler, not a universal schema VM or protocol
> runtime.

- Repeated sequences and arbitrary conditional fields are not supported.
- Prefix fields and bounded regions are sequential-only.
- Absolute layouts remain fixed-width and explicit-offset-only.
- The framework does not own checksums, semantic relationships, protocol state, I/O, or
  allocation policy.
- Custom codecs remain explicit Rust types rather than runtime descriptors.

For the normative ownership, parsing, mutation, and extension rules, see
[`ARCHITECTURE.md`](ARCHITECTURE.md). Generated APIs and codec contracts are documented
in the [crate documentation](https://docs.rs/wire-repr).

## 📦 Workspace and contract

The repository contains two crates:

- `wire-repr` — public `no_std` runtime facade and `wire_repr!` reexport;
- `wire-repr-macros` — host-side procedural-macro compiler.

The target-runtime contract is Rust 1.91, edition 2024, empty default features, no
allocation, no runtime dependencies, and `unsafe_code = "deny"`.

## 📄 License

MIT © 2026 SilentBless. See [LICENSE](LICENSE).
