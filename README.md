# wire-repr

**Byte layouts with real boundaries, not a runtime schema engine.**

`wire-repr` compiles a compact layout declaration into borrowed immutable views,
restricted mutable views, and builders for binary formats. Use it when bytes are
the source of truth—protocol headers, files, storage pages, firmware records, IPC—
and you want the compiler to keep offsets, lengths, and output writes honest.

The layout owns **physical representation**. Your application owns meaning: magic
values, reserved-bit policy, checksums, protocol state, and cross-field rules.

> [!IMPORTANT]
> Views borrow ordinary slices. They never reinterpret a byte buffer as a Rust
> struct, require alignment, allocate, use `unsafe`, or carry a runtime schema.

## ✨ Why this exists

Handwritten parsing is easy until framing and mutation meet: an offset moves, a
length is trusted too early, a write fails halfway through, or a prefix gets
silently canonicalized. `wire-repr` makes those structural operations generated
and explicit while leaving domain policy where it belongs: with the consumer.

- **Borrow directly.** Immutable views retain the exact accepted representation.
- **Frame explicitly.** Choose one representation plus a suffix, or reject suffixes.
- **Mutate narrowly.** Generated setters cannot resize or invalidate framing.
- **Build atomically.** Any builder error leaves all caller output unchanged.
- **Stay small.** The target library is `no_std`, `no_alloc`, safe Rust, and has no
  target-runtime dependencies.

## 📦 Installation

```toml
[dependencies]
wire-repr = { version = "0.4", default-features = false }
```

The package exports the `wire_repr!` macro and built-in codecs such as `U8`,
`BeU16`, `LeU32`, and `bytes(N)`.

## 🚀 Start here: one header, exact or framed

A declaration generates the immutable `Header<'wire>` type itself. `Header::view`
creates a lightweight request; the chosen terminal performs parsing and framing.

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
let (header, remainder) = Header::view(&input)
    .with_remainder()
    .expect("valid header");

assert_eq!(header.as_bytes(), &input[..4]);
assert_eq!(remainder, &[0xff]);
assert_eq!(header.kind(), 7);
assert_eq!(header.length(), 256);
assert!(header.enabled());
assert_eq!(header.mode(), 5);
```

Use `.without_trailing()` when the whole input must be this representation:

```rust
let exact = [7, 0x01, 0x00, 0b0000_1011];
let header = Header::view(&exact)
    .without_trailing()
    .expect("exact header");
assert_eq!(header.as_bytes(), &exact);
```

> [!NOTE]
> `with_remainder()` validates one bounded representation and returns the bytes
> after it. The suffix is not part of `as_bytes()`. `without_trailing()` performs
> the same structural validation and then rejects any suffix.

## 🧭 Parsing layouts

### Fixed sequential layouts

A normal layout is sequential. Fields, padding, and alignment entries occupy
contiguous physical positions in declaration order:

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

Use `position` only when physical wire order differs from API order. Then **every**
physical entry needs a contiguous one-based position. Declaration order still owns
getter, builder, and rustdoc order.

```rust
wire_repr::wire_repr! {
    pub layout Reordered {
        field checksum: BeU16 { position: 2; }
        field tag: U8 { position: 1; }
    }
}
```

Padding and alignment are represented opaque bytes. They are not reserved-field
semantics; use `bytes(N)` for named bytes your application wants to inspect.

### Fixed values, bytes, and projections

Built-in fixed codecs cover unsigned 8/16/24/32/64/128-bit and signed
8/16/32/64/128-bit integers in their applicable byte orders. Fixed codecs decode
every width-valid bit pattern. `bytes(N)` returns the original borrowed `&[u8]`.

Unsigned built-in integer storage can expose bit projections:

```rust
field flags: U8 {
    projections {
        bit enabled: 0;
        bits mode: 1..=3;
    }
}
```

Bit zero is the least-significant bit of the decoded integer regardless of wire
endianness. A projection owns no bytes and has no runtime metadata or dispatch.

### Prefix fields

A custom `PrefixCodec` determines a sequential field's encoded width during
structural parsing:

```rust
field name: prefix(crate::Terminated);
```

The view stores the accepted extent. `name()` decodes it; `name_raw()` returns the
exact validated bytes, including a legal noncanonical encoding. Prefix codecs are
responsible for bounded prefix validation; the generated parser rejects a claimed
extent beyond remaining input.

## ✍️ Mutable views

Immutable parsing is `Layout::view(bytes)`. Mutable parsing stays deliberately
separate:

```rust
let mut bytes = [7, 0x01, 0x00, 0b0000_1011];
let mut header = HeaderViewMut::parse_exact_mut(&mut bytes).expect("valid header");
header.set_kind(9).expect("encodable kind");
header.set_flags(0b0000_0010).expect("encodable flags");
```

Use `parse_prefix_mut` when caller bytes may have a suffix and `parse_exact_mut`
when they may not. A mutable view preserves the same represented extent that an
immutable view would accept.

Only same-width fixed fields that cannot affect framing receive typed setters.
Prefix fields, dynamic ranges, `buf_end`, and range-source fields have no setter.
A dynamic range exposes an exact mutable slice instead, so it can be edited but
not resized or reframed.

## 🏗️ Builders: validate first, write second

Builders write into caller-owned storage and return the bounded mutable view plus
its untouched suffix:

```rust
let mut output = [0xa5; 6];
let (header, suffix) = HeaderBuilder::new()
    .kind(7)
    .length(256)
    .flags(0b0000_1011)
    .build_into(&mut output)
    .expect("complete builder and sufficient output");

assert_eq!(header.as_bytes(), &[7, 0x01, 0x00, 0b0000_1011]);
assert_eq!(suffix, &[0xa5, 0xa5]);
```

Before the first output byte changes, a builder checks required inputs, codec
plans, derived values, dynamic lengths, source conversions, arithmetic,
shared-source agreement, and capacity. If any of that fails, **the entire supplied
output slice is unchanged**. On success, only the representation is written;
padding, alignment, absolute-layout gaps, and the suffix retain their prior bytes
unless the representation explicitly covers them.

## 📏 Dynamic byte ranges

Sequential layouts support three forms:

| Form | Meaning |
| --- | --- |
| `bytes(current_pos..current_pos + source)` | a length relative to the range start |
| `bytes(current_pos..source)` | an exclusive endpoint relative to representation byte zero |
| `bytes(current_pos..buf_end)` | the rest of the supplied view buffer |

A relative range derives its builder source from payload length:

```rust
wire_repr::wire_repr! {
    pub layout Frame {
        field payload_length: BeU16;
        field payload: bytes(current_pos..current_pos + payload_length);
        field checksum: BeU32;
    }
}

let mut output = [0; 9];
let view = FrameBuilder::new()
    .payload(b"abc")
    .checksum(0xfeed_beef)
    .build_into(&mut output)
    .expect("complete frame")
    .0;
assert_eq!(view.payload_length(), 3);
```

The first two forms require an eligible **physically preceding** fixed built-in
integer source, optionally total-mapped. The parser uses its raw physical value
and a checked `usize` conversion. `U24` therefore uses `u32`; a semantic wrapper
does not change geometry. Dynamic ranges may be empty; `bytes(0)` is not valid.

For `current_pos..source`, `source` is an exclusive endpoint from representation
byte zero, not a length. Builders derive that physical endpoint, including earlier
fixed and prefix widths, padding, alignment, and ranges. Later physical entries
still belong to the representation and still affect its remainder.

`buf_end` has no source, may appear once, and must be physically last. It owns all
remaining supplied input, including an empty span. It makes `with_remainder()`
return an empty suffix; it does **not** discover a packet, transport, or checksum
boundary for you.

## 🗺️ Absolute layouts

Absolute layouts are fixed width and require zero-based offsets for every field:

```rust
wire_repr::wire_repr! {
    pub absolute layout DatabaseHeader {
        field magic: bytes(16) { offset: 0; }
        field version: BeU32 { offset: 16; }
    }
}
```

Their width is the largest field extent. Gaps are represented bytes: views retain
them and builders preserve their existing output contents. Offsets are not inferred,
and padding, alignment, prefix fields, and dynamic ranges do not apply. Runtime
codec-width overlap is rejected before input slicing.

## 🧩 Custom codecs, mappings, and generated values

### Custom codecs

Implement `FixedCodec` for a compile-time-width representation or `PrefixCodec`
for a structurally validated sequential prefix. `plan` produces an `EncodePlan`;
its fallible work occurs before builder writes. A successful plan must encode the
planned value, fixed plans must match their declared width, and prefix plans must
be nonempty.

### Total mappings and declared scalars

`as TypePath` is a total API mapping over an eligible built-in fixed integer or
`bytes(N)` field—not a new codec:

```rust
field kind: BeU16 as crate::Kind;
field address: bytes(4) as crate::Address;
```

It requires `Type: From<Raw>` and `Raw: From<Type>`. The generated field has both
semantic and raw forms (`kind()`/`kind_raw()`, semantic/raw setters, and builder
inputs); the last supplied builder form wins. Mapped bytes use owned `[u8; N]`
values, while unmapped `bytes(N)` remains borrowed. Mappings do not apply to
custom, prefix, declared-scalar, or range fields.

`scalar Name: Codec;` instead declares a reusable nominal wrapper around a codec.
Use it when the codec itself—not merely one field's API—is the named concept.

### Derivations, contexts, and finalizers

A `derive` field is calculated during builder preflight and can fail. It has no
ordinary input or setter. Builder-only `context` values are explicit borrowed
inputs; they are neither bytes nor parser/view state.

A `finalize` field is different: after all fallible work, the builder writes its
target as zero and runs infallible finalizers in compile-time dependency order.
Finalizers may read declared values, contexts, and represented bytes, but cannot
rewrite arbitrary spans. Their `buf_end` is the represented extent—not the caller's
whole output—and target eligibility is intentionally narrow: direct, unmapped,
unprojected built-in fixed integers with an infallible encoding (not `U24`).

## 🔬 What reaches the CPU

Generated code is ordinary direct Rust: bounded slice access, endian conversion,
shifts/masks, and bounded copies. There are no runtime descriptors, schema walks,
erased codecs, allocation, or dynamic dispatch.

The probes below live beside equivalent handwritten Rust in
[`wire-repr/tests/codegen.rs`](wire-repr/tests/codegen.rs). The assembly was captured
with Rust 1.91.0 (`f8297e351`), LLVM 21.1.2, `--release`, targeting
`x86_64-unknown-linux-gnu`. Compiler-local labels are shortened; the instructions
are unchanged.

### [Fixed getter probe](wire-repr/tests/codegen.rs)

```rust
fn read_word(bytes: &[u8]) -> Option<u16> {
    CodegenPacket::view(bytes)
        .without_trailing()
        .ok()
        .map(|view| view.word())
}
```

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

### [Bit projection probe](wire-repr/tests/codegen.rs)

```rust
fn read_low_bit(bytes: &[u8]) -> Option<bool> {
    CodegenPacket::view(bytes)
        .without_trailing()
        .ok()
        .map(|view| view.word_low())
}
```

```asm
movb    $2, %al
cmpq    $2, %rsi
jne     .done
movzbl  1(%rdi), %eax
andb    $1, %al
.done:
retq
```

### [Same-width mutation probe](wire-repr/tests/codegen.rs)

```rust
fn replace_word(bytes: &mut [u8], value: u16) -> bool {
    match CodegenPacketViewMut::parse_exact_mut(bytes) {
        Ok(mut view) => view.set_word(value).is_ok(),
        Err(_) => false,
    }
}
```

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

### [Fixed builder probe](wire-repr/tests/codegen.rs)

```rust
fn write_word(output: &mut [u8], value: u16) -> bool {
    CodegenPacketBuilder::new()
        .word(value)
        .build_into(output)
        .is_ok()
}
```

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

### [Relative-range getter probe](wire-repr/tests/codegen.rs)

```rust
fn first_payload_byte(bytes: &[u8]) -> Option<u8> {
    CodegenRelativeRange::view(bytes)
        .without_trailing()
        .ok()?
        .payload()
        .first()
        .copied()
}
```

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

Snapshots are illustrative and compiler-specific. The normative release check is
[`ci/check-codegen.py`](ci/check-codegen.py), which compares the probes with their
handwritten safe-Rust equivalents and rejects unwanted calls, panic paths,
allocation, dynamic dispatch, or excess instruction shape.

## ✅ Guarantees and limits

**Guaranteed:** safe Rust only; exact accepted raw prefix bytes; explicit framing;
checked dynamic endpoints; immutable views that retain their validated boundaries;
restricted in-place mutation; and all-or-nothing caller-output builds.

**Deliberately absent:** repeated sequences, arbitrary conditional fields, nested
range schemas, independently owned bitfields, runtime schemas, allocation policy,
I/O, protocol state, and domain validation. Prefix fields and ranges are
sequential-only; absolute layouts are fixed-width only.

> [!TIP]
> If a variable-width value such as ULEB128 must frame a later range, retain that
> framing in consumer code. Prefix codecs are structural fields, not range sources.

## 🧬 Code generation and project contract

The workspace has two crates:

- [`wire-repr`](wire-repr/) — public runtime facade, codec contracts, and macro reexport;
- [`wire-repr-macros`](wire-repr-macros/) — host-side procedural-macro compiler.

The macro emits concrete layout-specific types and operations; it emits no generated
source files or build scripts. The target contract is **Rust 1.91**, edition 2024,
empty default features, `no_std`, no allocation requirement, no target-runtime
dependencies, and `unsafe_code = "deny"`.

For normative ownership and generation rules, read
[`ARCHITECTURE.md`](ARCHITECTURE.md). Public API details are also available on
[docs.rs](https://docs.rs/wire-repr).

## 📄 License

MIT © 2026 SilentBless. See [LICENSE](LICENSE).
