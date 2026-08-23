<h1 align="center">wire-repr</h1>

<p align="center">
  <strong>Compile Rust wire schemas into zero-copy views and atomic writers.</strong>
</p>

<p align="center">
  <code>no_std</code> · no allocation · safe Rust · Rust 1.91
</p>

`wire-repr` derives binary representation code from ordinary Rust structs and enums.
The declared type is the semantic value used for writing. Reading produces either a
borrowing `FooView<'wire>` or, with the `bytes` feature, a lifetime-free view over shared
backing storage.

## 📦 Add it

```toml
[dependencies]
wire-repr = { version = "1", default-features = false }
```

Enable `bytes` when decoded views need to own shared input instead of borrowing it:

```toml
[dependencies]
wire-repr = { version = "1", features = ["bytes"] }
bytes = { version = "1", default-features = false }
```

The schema stays unchanged. `Type::view(bytes::Bytes)` then returns a lifetime-free,
cloneable view over the same backing storage. Builders and prepared plans still borrow
semantic inputs; encoding appends into caller-owned, pre-capacitated `bytes::BytesMut`
without reserving or staging a second frame.

## 🚀 A real header

A Bitcoin block header is exactly 80 bytes:

```rust
use wire_repr::Wire;

#[derive(Debug, Eq, PartialEq, Wire)]
struct BitcoinHeader {
    #[wire(le)]
    version: i32,
    previous_block_hash: [u8; 32],
    merkle_root: [u8; 32],
    #[wire(le)]
    timestamp: u32,
    #[wire(le)]
    target_bits: u32,
    #[wire(le)]
    nonce: u32,
}

let bytes = [
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

let header = BitcoinHeader::view(&bytes)
    .without_trailing()
    .expect("genesis header is structurally valid");
assert_eq!(header.version(), 1);
assert_eq!(header.timestamp(), 1_231_006_505);
assert_eq!(header.nonce(), 2_083_236_893);
assert_eq!(header.as_bytes(), &bytes);
```

The view borrows the original bytes. It does not allocate, copy the header, or pretend
that a Rust struct has a stable wire ABI. Proof of work, hashes, display byte order, and
Bitcoin policy remain consumer code.

## 🧩 Struct fields

Fields are represented in declaration order.

- Plain `u8` and `i8` are one byte.
- `#[wire(be)]` and `#[wire(le)]` select the byte order of multibyte integers.
- `[u8; N]` is a fixed borrowed byte array in the generated view.
- A nested `Wire` type produces its nested generated view.
- `#[wire(codec = Path)]` uses a custom fixed-width `FixedCodec`.
- `#[wire(prefix = Path)]` uses a self-delimiting `PrefixCodec`.
- `#[wire(bytes = source)]` borrows the number of bytes decoded from an earlier
  unsigned source field. Preparation derives the canonical source value from the slice.
- `#[wire(rest)]` borrows the terminal remainder.
- `pad_before`, `align_before`, and forward-only `at` describe physical gaps. Input gap
  bytes are opaque; prepared output writes zeroes.

A successful `Foo::view(input)` terminal validates the complete geometry once. Getters
then decode values from retained exact spans; dynamic endpoints are not rediscovered.

## 🔖 Tagged operations

Static tagged enums use an explicit tag codec, explicit unknown policy, and unit or
one-field tuple variants:

```rust
use wire_repr::Wire;

#[derive(Debug, Eq, PartialEq, Wire)]
struct Ping {
    #[wire(be)]
    sequence: u16,
}

#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(tag = U8, unknown = reject)]
#[repr(u8)]
enum Message {
    Ping(Ping) = 1,
    Halt = 2,
}

let message = Message::view(&[1, 0x12, 0x34])
    .without_trailing()
    .expect("known message is structurally valid");
assert_eq!(message.ping().unwrap().sequence(), 0x1234);
```

Fixed byte selectors use their byte array as the tag representation. An open enum can
preserve unknown tags losslessly — useful for formats such as PNG, where extensions are
valid wire values:

```rust
use wire_repr::Wire;

#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(tag = [u8; 4], unknown = preserve)]
enum ChunkType {
    #[wire(tag = b"IHDR")]
    Ihdr,
    #[wire(tag = b"IEND")]
    Iend,
    #[wire(unknown)]
    Other([u8; 4]),
}

let chunk_type = ChunkType::view(b"vpAg")
    .without_trailing()
    .expect("unknown chunk type remains representable");
assert_eq!(chunk_type.other(), Some(b"vpAg"));
```

For negotiated numeric IDs, a schema names one concrete consumer-owned operation input.
The declared name is also the generated fluent method and the explicit forwarding marker.
For example, `opcodes = Type`, per-variant `opcodes = Path`, and `#[wire(opcodes)]` fields
generate:

```rust
Packet::view(bytes).opcodes(&opcodes).without_trailing()
packet.opcodes(&opcodes).prepare()
```

The name is schema-defined except for wire options and generated API method names:
`table = Type` generates `.table(&table)`, while `offsets = Type` generates
`.offsets(&offsets)`. The input maps raw IDs in both directions, is forwarded only by matching
schema names, and is not retained in the generated view or prepared plan.

## 🚩 Nominal bitfields

A bitfield is its own semantic type and its own physical owner:

```rust
use wire_repr::Wire;

#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(bitfield = u16, be, reserved = zero)]
struct Flags {
    #[wire(bit = 0)]
    enabled: bool,
    #[wire(bits = 1..=3)]
    mode: u8,
}

let flags = Flags::view(&[0, 0b0000_1011])
    .without_trailing()
    .expect("flags are structurally valid");
assert!(flags.enabled());
assert_eq!(flags.mode(), 5);
```

Bit numbers are semantic least-significant-bit positions after byte-order decoding.
Unprojected bits are accepted on read and written as zero by the explicit
`reserved = zero` policy.

## 🔁 Sequences

Statically fixed records expose an ordinary infallible, exact-size iterator after one
framing check:

```rust
let records = Header::views(bytes).expect("records have fixed-width framing");
for record in records {
    use_record(record);
}
```

Potentially variable-width records expose a fail-closed cursor:

```rust
let mut records = Chunk::cursor(bytes);
while let Some(record) = records.next().expect("next record is structurally valid") {
    use_record(record);
}
```

A cursor never advances past a malformed item. This keeps variable item errors explicit
without allocating an index or pretending later boundaries were validated.

## 📥 Frame one view

`Type::view(input)` starts one framing request:

- `.with_remainder()` returns one validated `TypeView<'wire>` and the disjoint suffix.
- `.without_trailing()` returns one view and rejects trailing bytes.

`view.as_bytes()` is exactly the represented input range. The semantic Rust value is not
materialized during reading; use generated getters.

## 📤 Prepare, then write

Encoding consumes the ordinary semantic value:

```rust
use wire_repr::{PreparedLayout, Wire};

#[derive(Wire)]
struct Header {
    kind: u8,
}

let plan = Header { kind: 7 }
    .prepare()
    .expect("header preparation succeeds");
let mut output = [0xa5; 2];
let (written, suffix) = plan
    .commit_into(&mut output)
    .expect("output has enough capacity");
assert_eq!(written.as_bytes(), &[7]);
assert_eq!(suffix, &mut [0xa5]);
```

Preparation completes fallible codec planning, conversions, geometry, canonical length
sources, operation-input mapping, and total-length arithmetic before output mutation. Commit
checks full capacity before its first write. A short output remains byte-for-byte
unchanged.

## 🧮 Select and compute physical bytes

Prepared plans and bytes-backed views expose typed, allocation-free selections of their
physical representation:

```rust
let covered = view.bytes().include(|fields| fields.header | fields.payload);
let signed = view.bytes().exclude(|fields| fields.signature);
```

Selections stay in physical wire order. Nested paths are supported, and exclusions preserve
gaps and padding. A fragmented selection is a `ByteSource`: it can stream directly to a sink
or expose borrowed and virtual repeated-byte segments through `ByteSourceCursor` without
building a temporary packet-sized buffer.

Computed fields use the same source contract during preparation:

```rust
use wire_repr::{ByteSourceCursor, Wire};

fn checksum(source: &impl ByteSourceCursor) -> u8 {
    source.bytes().fold(0, u8::wrapping_add)
}

#[derive(Wire)]
struct Packet<'wire> {
    #[wire(computed = checksum(exclude(self)))]
    checksum: u8,
    length: u8,
    #[wire(bytes = length)]
    payload: &'wire [u8],
}
```

The selection is the computation's compile-time read-set, so dependencies are ordered
without relying on declaration order and cycles are rejected by the derive. Computations are
infallible derivations; preparation checked-converts their results into the stored field type.
Reading still returns the stored computed value. If stored-value consistency matters, validate
it separately against the exact-source view selection.

`wire_repr::computation::len` is a generic slice-length helper using this same callback path;
it has no macro privileges and can be replaced by an ordinary function. A field referenced by
`#[wire(bytes = length)]` is different: the framing relation derives `length` canonically from
the payload extent during preparation, so that source cannot also declare a computation.

## 🧪 Real formats

The repository includes consumer fixtures for [PNG chunks](wire-repr/tests/consumer_formats/png.rs),
[SQLite headers](wire-repr/tests/consumer_formats/sqlite.rs), and
[WebAssembly sections](wire-repr/tests/consumer_formats/wasm.rs). They keep magic values,
checksums, and format policy outside the representation layer.
