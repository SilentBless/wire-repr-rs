# Borrowed wire views and atomic writers

`wire-repr` derives safe, `no_std`, allocation-free binary representation code from
ordinary Rust structs and enums.

A declared `Foo` is the semantic value used for writing. Reading produces a generated
`FooView<'wire>` that borrows the original bytes, validates their geometry once, and
exposes getters. Protocol policy remains consumer code.

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

let input = [7, 3, 10, 11, 12, 0x12, 0x34, 0xaa];
let (packet, suffix) = Packet::view(&input).with_remainder()?;
assert_eq!(packet.kind(), 7);
assert_eq!(packet.payload(), &[10, 11, 12]);
assert_eq!(packet.sequence(), 0x1234);
assert_eq!(packet.as_bytes(), &input[..7]);
assert_eq!(suffix, &[0xaa]);
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

let command = Command::view(&[1, 0x12, 0x34]).without_trailing()?;
assert!(!command.is_halt());
assert_eq!(command.ping().unwrap().id(), 0x1234);
# Ok::<(), CommandDecodeError>(())
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

let chunk_type = ChunkType::view(b"vpAg")
    .without_trailing()
    .expect("unknown tag is preserved");
assert_eq!(chunk_type.other(), Some(b"vpAg"));
```

Unknown tags are rejected by the explicit policy. Dynamic numeric IDs can instead use a
consumer-owned opcode table supplied with `.opcodes(&table)` during validation and
preparation.

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

let flags = Flags::view(&[0b1110_1011]).without_trailing()?;
assert!(flags.enabled());
assert_eq!(flags.mode(), 5);
# Ok::<(), FlagsDecodeError>(())
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

let mut words = Word::views(&[0x12, 0x34, 0xab, 0xcd])?;
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

let mut output = [0xa5; 4];
let (written, suffix) = plan.commit_into(&mut output)?;
assert_eq!(written.as_bytes(), &[3, 1, 2]);
assert_eq!(suffix, &mut [0xa5]);
# Ok::<(), Box<dyn core::error::Error>>(())
```

Preparation completes all fallible codec work, conversions, opcode mapping, dynamic
geometry, and total-length checks before output mutation. Commit checks capacity before
its first write. Short output remains byte-for-byte unchanged.

For custom field codecs and exact planning contracts, see [`codec`].
