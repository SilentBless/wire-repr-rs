<h1 align="center">wire-repr</h1>

<p align="center"><strong>Compile Rust wire schemas into exact-source views and progressive writers.</strong></p>

<p align="center"><code>no_std</code> · no allocation · safe generated API · Rust 1.91</p>

`wire-repr` is for binary formats whose bytes remain the source of truth: network packets, file
records, storage pages, firmware messages, and IPC frames. A Rust schema describes physical layout.
The derives generate:

- a retained immutable view over borrowed or owned bytes;
- an output-owning typestate writer that emits the representation progressively.

The library owns widths, byte order, geometry, framing, controllers, selectors, and exact byte
ranges. Your application keeps ownership of protocol meaning and semantic objects.

## Install

```toml
[dependencies]
wire-repr = "1"
```

The crate has no Cargo features. Slices, arrays, `Vec<u8>`, `bytes::Bytes`, and custom
`AsRef<[u8]>` backings all use the same view API. Growable outputs are selected by their ordinary
Rust capabilities rather than by a wire-repr feature.

## Quick start

```rust
use wire_repr::{WireBuilder, WireView, wire};

#[derive(WireView, WireBuilder)]
struct Packet {
    #[wire(be, constant = 0x5752)]
    magic: u16,
    kind: u8,
    #[wire(be)]
    payload_len: u16,
    #[wire(bytes = payload_len)]
    payload: wire::Bytes,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = [0x57, 0x52, 7, 0, 5, b'h', b'e', b'l', b'l', b'o'];

    let packet = Packet::view(&input)?;
    assert_eq!(packet.magic(), 0x5752);
    assert_eq!(packet.kind(), 7);
    assert_eq!(packet.payload(), b"hello");
    assert_eq!(packet.as_bytes(), input);

    let mut output = Vec::new();
    let written = Packet::builder(&mut output)
        .kind(7)?
        .payload(&b"hello"[..])?
        .finish()?;

    assert_eq!(written.as_bytes(), input);
    Ok(())
}
```

`payload_len` remains a real physical field. Reading trusts its stored value. Writing derives and
patches it from `payload`, so no length setter is generated. Constants behave the same way: they are
validated and exposed on views, then written automatically.

## Live network examples

The examples include actual network round trips rather than only embedded byte fixtures:

```text
cargo run -p wire-repr --example dns -- example.com
cargo run -p wire-repr --example ntp
cargo run -p wire-repr --example telegram
```

[`dns.rs`](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.0/wire-repr/examples/dns.rs)
builds a DNS query, sends it through `UdpSocket`, frames the returned datagram, and prints its
flags and section counts.
[`ntp.rs`](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.0/wire-repr/examples/ntp.rs)
builds an NTPv4 client packet, validates the server response, and calculates the observed clock
offset.

DNS transaction IDs and echoed NTP origin timestamps correlate replies but do not authenticate
plaintext UDP. Their output is informational; consumers requiring trusted naming or time need the
appropriate authenticated protocol.

The advanced
[`telegram.rs`](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.0/wire-repr/examples/telegram.rs)
example opens a real intermediate-transport TCP connection to Telegram DC2. Its schema writes the
transport marker as a constant, derives both framing lengths, computes a fresh MTProto message ID,
and obtains the `int128` nonce through a fallible computed callback. The response is represented by
typed `resPQ`, short TL bytes, and fingerprint-vector views with schema validators—no hand-written
wire parser. It stops before factorization and Diffie-Hellman because receiving `resPQ` alone does
not authenticate the server.

These binaries use only `std` networking plus the dev-only `getrandom` dependency. Neither enters
the normal `wire-repr` target dependency graph. Every network read has a timeout and a declared
maximum frame size; command-line arguments can override the default resolver, time server, or
Telegram endpoint.

## Views retain your backing

`Schema::view<T: AsRef<[u8]>>(input: T)` stores `T` directly:

```rust
let borrowed = Packet::view(&bytes[..])?;
let owned = Packet::view(bytes.to_vec())?;
let shared = Packet::view(bytes::Bytes::copy_from_slice(bytes))?;
```

Passing a reference borrows. Passing a collection or shared handle moves it into the opaque view.
Nested getters borrow their parent and reconstruct only ranges already proved by framing. There is
no separate owned renderer, lifetime parameter on the schema, hidden allocation, or semantic object
materialization.

The retained backing must keep projecting the same immutable byte span while the view exists.
Slices, arrays, `Vec`, `bytes::Bytes`, and ordinary wrappers satisfy that contract; intentionally
stateful `AsRef` implementations do not.

## Writers own one progressive cursor

`Schema::builder(output)` writes fields as they become available. Setters consume and return
successive typestate stages, so `?` is the normal control flow and `finish()` exists only after every
required field has been supplied.

Output behavior follows the output type:

```rust
Packet::builder(&mut fixed_slice[..]);     // fixed; returns NeedMore
Packet::builder(&mut vec);                 // grows through Extend<u8>
Packet::builder(output::bounded(&mut vec, limit));
Packet::builder(output::grow_with(&mut arena, grow));
Packet::builder(output::owned(vec));       // movable through a 'static worker
```

Generated code keeps offsets rather than pointers, so growable storage may relocate safely.
`finish()` returns `Written<O>` with the exact represented range. `output::owned` returns the
caller-selected collection with that range and adds no allocator dependency to wire-repr.

Writing is deliberately progressive, not transactional. An error may leave partial unpublished
bytes. Applications needing atomic publication should build in an unpublished slot, staging buffer,
or double buffer and publish only after `finish()` succeeds.

## Layout capabilities

The `1.0` schema model includes:

| Capability | Schema surface |
| --- | --- |
| Scalars | fixed integers and floats, explicit endian, constants, fixed primitive arrays |
| Logical types | `as = Physical` with checked bidirectional `TryFrom`, including `NonZero*` and newtypes |
| Nested layouts | derived or manual children, generics, lifetimes, const generics, and `where` clauses |
| Dynamic geometry | bounded `wire::Bytes`, terminal `rest`, padding, alignment, and forward placement |
| Dependencies | shared length controllers, conditional groups, zero-width flags, and counted arrays |
| Collections | lazy `ArrayView`, `IntoIterator`, streaming `try_extend`, and exact range forwarding |
| Enums | selector-only unit variants, body variants, and exact bounded or terminal unknown bodies |
| Bitfields | reusable nominal bitfields and inline projections from earlier physical scalars |
| Physical selections | allocation-free root and nested byte ranges for copying and checksums |
| Computed fields | infallible or fallible callbacks ordered by a generated dependency DAG |
| Sequences | homogeneous `views()` and failure-atomic heterogeneous `Cursor` consumption |
| Recursion | depth-bounded enum arrays and object continuations with progressive recursive writers |

Unsupported or ambiguous layouts fail during derive expansion instead of selecting a heuristic
runtime interpretation. Runtime selector registries, mutable views, async I/O, semantic object
mapping, hidden collection indexes, and general traversal are outside the `1.0` core.

## Collections and exact forwarding

A runtime collection keeps its controller visible in the schema:

```rust
#[derive(WireView, WireBuilder)]
struct Items<T> {
    #[wire(le)]
    count: u16,
    #[wire(counted_by = count)]
    values: wire::Array<T>,
}
```

`ArrayView` retains the collection range and authoritative count, not item offsets. Iteration keeps
one forward cursor. `get(n)` replays only as much geometry as the selected representation requires.
Writers stream arbitrary Rust iterators:

```rust
packet.values(|values| {
    values.try_extend(source, |item, value| item.value(value))
})?;
```

When the source is already a validated wire array, the writer can forward it as one exact range:

```rust
packet.values(|values| values.copy_from(source.values()))?;
```

## Enums, bitfields, and computed bytes

Static enums expose an ordinary borrowed Rust variant enum. Unit variants write only their selector;
body variants use closure setters. An explicit `#[wire(unknown)]` variant preserves the raw selector
and exact body for lossless forwarding.

Nominal bitfields declare their physical integer on the type and logical ranges on fields. Fresh
writers zero undeclared bits; copying an exact view preserves every source bit.

Computed fields can consume logical getters and physical selections without flattening bytes:

```rust
#[wire(be, computed = checksum(exclude(self)))]
checksum: u16,
```

`select(&view)` also exposes ordered, merged `chunks()` and byte iteration through nested typed
field paths. The
[expression VM](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.0/wire-repr/examples/expression_vm.rs)
wraps recursive bytecode in a dynamic representation whose FNV-1a digest is generated from exact
physical ranges and checked again by a schema validator.

## Recursive layouts

Closed selector enums may recurse through counted arrays or object fields marked with
`wire::Recursive<T>`. The caller chooses a const depth bound:

```rust
let value = Value::view::<64>(input)?;
```

Generated framing uses an iterative typed continuation stack. Recursive arrays retain bounded exact
geometry rather than item offsets; unsupported compression modes fall back to exact replay.
Progressive recursive writers stream children through the same output cursor and retain no semantic
tree or encoded plan.

The runnable
[recursive expression VM](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.0/wire-repr/examples/expression_vm.rs)
compiles `(20 + 22) * -2` through the progressive writer, wraps it with a computed and validated
digest, then evaluates the retained recursive child views.
[`ARCHITECTURE.md`](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.0/ARCHITECTURE.md)
documents the compact geometry and continuation invariants for maintainers.

## Errors and incomplete input

Generated errors are nominal field-site enums. Nested failures keep their concrete source type and
read errors carry absolute root-input offsets. Incomplete contiguous input returns:

```rust
NeedMore {
    offset,
    additional_at_least,
}
```

The caller owns buffering and retries. `wire-repr` does not retain resumable parser state or own
`Read`/`AsyncRead`.

## Examples

From a repository checkout, all examples are executable with the pinned Rust 1.91 toolchain:

```text
cargo run -p wire-repr --example dns -- example.com
cargo run -p wire-repr --example ntp
cargo run -p wire-repr --example telegram
cargo run -p wire-repr --example expression_vm
```

- [`dns.rs`](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.0/wire-repr/examples/dns.rs)
  — live DNS query and response framing over UDP.
- [`ntp.rs`](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.0/wire-repr/examples/ntp.rs)
  — live NTPv4 request, timestamp validation, and clock-offset calculation.
- [`telegram.rs`](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.0/wire-repr/examples/telegram.rs)
  — real MTProto auth bootstrap against Telegram DC2.
- [`expression_vm.rs`](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.0/wire-repr/examples/expression_vm.rs)
  — recursive binary compiler, computed digest, validator, and evaluator.

The full API guide is on [docs.rs](https://docs.rs/wire-repr). The detailed internal contract is
in [`ARCHITECTURE.md`](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.0/ARCHITECTURE.md).

## Verification

The repository verifies behavior and generated code independently:

```text
cargo +1.91.0 test --workspace --all-targets
cargo +1.91.0 clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo +1.91.0 doc --workspace --no-deps
python3 ci/check-fail-fast.py
cargo +1.91.0 run -p wire-repr-measure --release -- run
```

The product-owned measurement corpus compares generated, idiomatic handwritten, and best-safe
implementations using final linked artifacts and calibrated runtime samples. LLVM instruction counts
remain diagnostic evidence rather than a latency oracle.

## License

Licensed under the
[MIT License](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.0/LICENSE).
