# Wire views and atomic writers

`wire-repr` derives safe, `no_std`, allocation-free binary representation code from
ordinary Rust structs and enums.

A declared `Foo` is the semantic value used for writing. By default, reading produces a
generated `FooView<'wire>` that borrows the original bytes. With the `bytes` feature, the
same API produces a lifetime-free view owning a shared `bytes::Bytes` handle. Both forms
validate geometry once and expose the same semantic getters. Protocol policy remains
consumer code.

## Read one representation

```rust
use wire_repr::Wire;

#[derive(Debug, Eq, PartialEq, Wire)]
struct Packet<'value> {
    kind: u8,
    length: u8,
    #[wire(bytes = length)]
    payload: &'value [u8],
    #[wire(be)]
    sequence: u16,
}

# #[cfg(not(feature = "bytes"))]
# let input = &[7, 3, 10, 11, 12, 0x12, 0x34, 0xaa][..];
# #[cfg(feature = "bytes")]
# let input = bytes::Bytes::from_static(&[7, 3, 10, 11, 12, 0x12, 0x34, 0xaa]);
let (packet, suffix) = Packet::view(input).with_remainder()?;
assert_eq!(packet.kind(), 7);
assert_eq!(packet.payload(), &[10, 11, 12]);
assert_eq!(packet.sequence(), 0x1234);
assert_eq!(packet.as_bytes(), &[7, 3, 10, 11, 12, 0x12, 0x34]);
assert_eq!(suffix.as_ref(), &[0xaa]);
# Ok::<(), PacketDecodeError>(())
```

`without_trailing()` validates the same leading representation and rejects a nonempty
suffix. A `#[wire(rest)]` slice consumes the terminal remainder.

## Tagged enums

```rust
use wire_repr::Wire;

#[derive(Debug, Eq, PartialEq, Wire)]
struct Ping {
    #[wire(be)]
    id: u16,
}

#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(tag = U8, unknown = reject)]
#[repr(u8)]
enum Command {
    Ping(Ping) = 1,
    Halt = 2,
}

# #[cfg(not(feature = "bytes"))]
# let input = &[1, 0x12, 0x34][..];
# #[cfg(feature = "bytes")]
# let input = bytes::Bytes::from_static(&[1, 0x12, 0x34]);
let command = Command::view(input).without_trailing()?;
assert!(!command.is_halt());
assert_eq!(command.ping().unwrap().id(), 0x1234);
# Ok::<(), CommandValidationError>(())
```

Fixed byte tags preserve byte identity without integer or UTF-8 reinterpretation:

```rust
use wire_repr::Wire;

#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(tag = [u8; 4], unknown = preserve)]
enum ChunkType {
    #[wire(tag = b"IEND")]
    Iend,
    #[wire(unknown)]
    Other([u8; 4]),
}

# #[cfg(not(feature = "bytes"))]
# let input = &b"vpAg"[..];
# #[cfg(feature = "bytes")]
# let input = bytes::Bytes::from_static(b"vpAg");
let chunk_type = ChunkType::view(input)
    .without_trailing()
    .expect("unknown tag is preserved");
assert!(chunk_type.other().is_some());
assert_eq!(chunk_type.as_bytes(), b"vpAg");
```

Unknown tags are rejected by the explicit policy. Dynamic numeric IDs can instead use a
consumer-owned operation input whose schema name generates the fluent method: an `opcodes`
group uses `.opcodes(&table)`, while a `table` group uses `.table(&table)`. The input is
borrowed during validation or preparation and is not retained in views or plans.

## Nominal bitfields

```rust
use wire_repr::Wire;

#[derive(Debug, Eq, PartialEq, Wire)]
#[wire(bitfield = u8, reserved = zero)]
struct Flags {
    #[wire(bit = 0)]
    enabled: bool,
    #[wire(bits = 1..=3)]
    mode: u8,
}

# #[cfg(not(feature = "bytes"))]
# let input = &[0b1110_1011][..];
# #[cfg(feature = "bytes")]
# let input = bytes::Bytes::from_static(&[0b1110_1011]);
let flags = Flags::view(input).without_trailing()?;
assert!(flags.enabled());
assert_eq!(flags.mode(), 5);
# Ok::<(), FlagsError>(())
```

The generated `FlagsView` owns one physical scalar span. Unprojected bits are accepted on
read and canonicalized to zero during preparation.

## Consecutive records

Plain fixed-width records use an infallible exact-size iterator after one framing check:

```rust
use wire_repr::Wire;

#[derive(Wire)]
struct Word {
    #[wire(be)]
    value: u16,
}

# #[cfg(not(feature = "bytes"))]
# let input = &[0x12, 0x34, 0xab, 0xcd][..];
# #[cfg(feature = "bytes")]
# let input = bytes::Bytes::from_static(&[0x12, 0x34, 0xab, 0xcd]);
let mut words = Word::views(input)?;
assert_eq!(words.len(), 2);
assert_eq!(words.next().unwrap().value(), 0x1234);
assert_eq!(words.next().unwrap().value(), 0xabcd);
assert!(words.next().is_none());
# Ok::<(), wire_repr::FixedViewSequenceError>(())
```

Potentially variable layouts expose `Type::cursor(bytes)`. Its
`next() -> Result<Option<TypeView<'wire>>, _>` validates one item and does not advance on
failure.

## Prepare, then commit

```rust
use wire_repr::{PreparedLayout, Wire};

#[derive(Debug, Eq, PartialEq, Wire)]
struct Header {
    kind: u8,
    #[wire(be)]
    code: u16,
}

let plan = Header { kind: 3, code: 0x0102 }.prepare()?;
assert_eq!(plan.encoded_len(), 3);

# #[cfg(not(feature = "bytes"))]
# {
# let mut output = [0xa5; 4];
# let (written, suffix) = plan.commit_into(&mut output)?;
# assert_eq!(written.as_bytes(), &[3, 1, 2]);
# assert_eq!(suffix, &mut [0xa5]);
# }
# #[cfg(feature = "bytes")]
# {
let mut output = bytes::BytesMut::with_capacity(3);
let written = plan.commit_into(&mut output)?;
assert_eq!(written.as_bytes(), &[3, 1, 2]);
# }
# Ok::<(), Box<dyn core::error::Error>>(())
```

Preparation completes all fallible codec work, conversions, operation-input mapping, dynamic
geometry, and total-length checks before output mutation. Commit checks capacity before
its first write. Short output remains byte-for-byte unchanged. In `bytes` mode, commit appends
after any existing prefix without reserving; the caller can freeze the `BytesMut` after the
returned `Written` borrow ends.

## Select and compute represented bytes

Generated plans and views expose typed physical-byte selections. The result remains a
streaming source rather than a hidden contiguous buffer:

```rust
use wire_repr::{ByteSource, ByteSourceCursor, Wire};

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

let payload = [1, 2, 3];
let plan = Packet::builder()
    .payload(&payload)
    .prepare()
    .expect("packet preparation succeeds");
let selected = plan.bytes().exclude(|fields| fields.checksum);
let mut represented = [0; 4];
selected.write_into(&mut represented);
assert_eq!(represented, [3, 1, 2, 3]);
```

Selectors are evaluated in physical wire order and support nested field projections.
Computed selections also define compile-time dependencies; derive rejects self-inclusion,
missing or duplicate paths, and cycles. Views select exact stored bytes, while plans select
canonical prepared bytes. Computations are infallible derivations; preparation
checked-converts their results into the stored field type.

[`computation::len`] is an ordinary generic callback helper for semantic slice lengths. A
source referenced by `#[wire(bytes = length)]` is instead derived canonically from the bounded
payload extent and cannot also declare a computation.

For custom field codecs and exact planning contracts, see [`codec`].
