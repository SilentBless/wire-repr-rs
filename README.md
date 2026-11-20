<h1 align="center">wire-repr</h1>

<p align="center">
  <strong>Derive zero-copy readers and writers for binary formats with variable-length fields.</strong>
</p>

<p align="center"><code>no_std</code> · no allocation · works under <code>#![forbid(unsafe_code)]</code> · Rust 1.91</p>

Length prefixes, counted arrays, tagged unions, optional groups, checksums: describe the byte layout
once as a Rust struct or enum. `wire-repr` derives a view that reads fields straight from the
source bytes and a typestate writer that fills in lengths, counts, constants, and checksums for you.
The generated code is the bounds checks and loads you would write by hand.

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

There is no `magic` or `payload_len` setter. The reader checks the constant and trusts the length;
the writer emits the constant and patches the length from the payload it wrote. A schema cannot
drift out of sync with its own length fields.

## Install

```toml
[dependencies]
wire-repr = "1"
thiserror = { version = "2", default-features = false }
```

Generated error enums derive `thiserror::Error`, so schema crates declare it directly. Both crates
are `no_std`, and `wire-repr` has no Cargo features.

## What the compiler emits

`Packet::view` above against the same checks written by hand, `rustc 1.91.0 --release` on
`aarch64-apple-darwin`:

```rust
pub fn derived(input: &[u8]) -> Option<(u8, usize)> {
    let packet = Packet::view(input).ok()?;
    Some((packet.kind(), packet.payload().len()))
}

pub fn handwritten(input: &[u8]) -> Option<(u8, usize)> {
    let header: &[u8; 5] = input.get(..5)?.try_into().ok()?;
    if u16::from_be_bytes([header[0], header[1]]) != 0x5752 {
        return None;
    }
    let len = usize::from(u16::from_be_bytes([header[3], header[4]]));
    if input.len() - 5 != len {
        return None;
    }
    Some((header[2], len))
}
```

```asm
derived:                          handwritten:
    cmp   x1, #2                      subs  x9, x1, #5
    b.lo  .none                       b.lo  .none
    mov   x9, #0
    cmp   x1, #5
    b.lo  .store
    ldrh  w10, [x0]                   ldrh  w10, [x0]
    mov   w11, #21079                 mov   w11, #21079
    cmp   w10, w11                    cmp   w10, w11
    b.ne  .store                      b.ne  .none
    ldurh w9, [x0, #3]                ldurh w10, [x0, #3]
    rev   w9, w9                      rev   w10, w10
    lsr   w9, w9, #16                 lsr   w10, w10, #16
    add   x10, x9, #5
    cmp   x10, x1                     cmp   x9, x10
    b.ne  .none                       b.ne  .none
    ldrb  w10, [x0, #2]               ldrb  w9, [x0, #2]
    strb  w10, [x8, #8]               strb  w9, [x8, #8]
    str   x9, [x8, #16]               str   x10, [x8, #16]
    mov   w9, #1                      mov   w9, #1
.store:
    str   x9, [x8]                    str   x9, [x8]
    ret                               ret
```

The same loads, the same comparisons, no calls, no allocation. The derive keeps one extra length
check because it reports which field was truncated.

The repository measures every layout class this way. Each case has a derived implementation and an
independent handwritten one checked against the same oracle; CI inspects the linked binaries for
allocation and indirect calls and fails when a gated case is more than 10% slower than the
handwritten code. Median nanoseconds per call from one run on an Apple M4 Max:

| Case | Derived | Handwritten | Instructions (derived / handwritten) |
| --- | ---: | ---: | ---: |
| Fixed scalars, decode | 0.78 | 0.97 | 5 / 5 |
| Fixed scalars, build | 0.81 | 0.77 | 4 / 4 |
| Generic nested child, decode | 0.78 | 0.84 | 6 / 6 |
| Length-prefixed fields, build | 2.56 | 2.37 | 44 / 44 |
| Tagged enum, decode | 2.39 | 2.40 | 56 / 55 |
| Conditional fields, decode | 1.03 | 1.33 | 18 / 41 |
| Counted array of variable items, decode | 12.8 | 11.8 | 288 / 198 |
| Self-delimiting children, build | 12.5 | 20.1 | 86 / 91 |
| ULEB128-heavy record, decode | 213 | 179 | 1889 / 1264 |
| Byte-wise fragmented selection | 4.85 | 1.51 | 70 / 25 |
| Recursive JSON-like tree, iterate 200 nodes | 8326 | 2789 | 3683 / 361 |

Flat, nested, tagged, conditional, and counted layouts match handwritten code. Byte-wise selections
and recursive layouts currently cost two to three times more, and some recursive geometry forms
cost more than that. Reproduce the full 75-case report with
`cargo run -p wire-repr-measure --release -- run` (see
[CONTRIBUTING.md](https://github.com/SilentBless/wire-repr-rs/blob/master/CONTRIBUTING.md)).

## How it differs

| | wire-repr | [zerocopy] | [binrw] | [deku] |
| --- | --- | --- | --- | --- |
| Reading model | View over the source bytes | Reinterpret a fixed-layout struct in place | Decode into owned values | Decode into owned values |
| Variable-length fields | Length-prefixed bytes, counted arrays, tagged bodies, optional groups | Trailing slice only | Yes | Yes |
| Length and count fields on write | Derived and patched by the writer | Not modelled | Declared per field with `calc` | Refreshed by calling `update()` |
| Source bytes after reading | Always available; a view is its bytes | Available | Re-encoded from values | Re-encoded from values |
| Allocation | None | None | Owned collections | None for fixed-size types, otherwise `alloc` |
| Bit-level fields | Bit ranges inside an integer | No | Through separate bitfield crates | Arbitrary bit widths |

[zerocopy]: https://docs.rs/zerocopy
[binrw]: https://docs.rs/binrw
[deku]: https://docs.rs/deku

Pick `zerocopy` when every field has a fixed offset. Pick `binrw` or `deku` when you want ordinary
owned Rust values and do not need the exact source bytes back. Pick `wire-repr` when the format has
variable-length parts and the bytes stay the source of truth: forwarding and proxying, signatures
and checksums over received data, firmware and kernel paths without an allocator, and parsers where
the generated code must be auditable.

## What a schema can describe

- **Scalars and arrays:** fixed integers, `f32` and `f64`, explicit byte order, constants,
  `[u8; N]`, and fixed primitive arrays.
- **Variable-width integers:** ULEB128, SLEB128, VU128, and ZigZag, as values or as length and count
  controllers.
- **Logical types:** `as = Physical` with checked `TryFrom` in both directions, including
  `NonZero*` and your own newtypes.
- **Geometry:** length-bounded bytes, terminal `rest`, padding, alignment, and forward placement.
- **Nested schemas:** fixed, length-bounded, or self-delimiting children, generic over the child
  type.
- **Dependencies:** shared length fields, flag-controlled optional groups, bit projections, and
  counted arrays whose items are themselves variable-width.
- **Enums and bitfields:** a physical selector with unit and body variants, an `unknown` variant
  that forwards unrecognized bodies byte for byte, and nominal bitfields.
- **Computed fields:** checksums and digests over selected byte ranges, written in an order derived
  from their dependencies; a schema validator can verify them on read.
- **Sequences:** iterators over repeated records and cursors over mixed record types.
- **Recursive layouts:** trees with a caller-chosen depth bound and no recursion in generated code.

The [crate documentation](https://docs.rs/wire-repr) is the schema reference: every attribute, the
ownership rules for views and outputs, the error contract, and the manual extension traits.

## Views and writers in one minute

`Schema::view(input)` accepts any `AsRef<[u8]>` and stores it as given. A reference borrows; a
`Vec` or `bytes::Bytes` moves into the view. Getters decode on demand and nested getters borrow
their parent.

```rust
let borrowed = Packet::view(&bytes[..])?;
let owned = Packet::view(bytes.to_vec())?;
```

`Schema::builder(output)` writes fields in physical order as you set them. Each setter returns the
next typestate, so a missing field is a compile error and `finish()` returns the exact written
range.

```rust
Packet::builder(&mut fixed_slice[..]);              // fixed; reports NeedMore
Packet::builder(&mut vec);                          // grows
Packet::builder(output::bounded(&mut vec, limit));  // grows up to a limit
```

Truncated input returns `NeedMore { offset, additional_at_least }` with absolute offsets. Buffering
and retries stay with the caller.

> [!WARNING]
> Writing is progressive, not transactional. An error may leave partial bytes in the output.
> Publish only the `Written` result.

## Examples against real servers

```text
cargo run -p wire-repr --example dns -- example.com
cargo run -p wire-repr --example ntp
cargo run -p wire-repr --example telegram
cargo run -p wire-repr --example expression_vm
```

- **[DNS over UDP](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.1/wire-repr/examples/dns.rs):**
  builds a query, sends it to the system resolver, and frames the response.
- **[NTPv4 over UDP](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.1/wire-repr/examples/ntp.rs):**
  builds a client packet, validates the server timestamps, and computes the clock offset.
- **[Telegram MTProto over TCP](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.1/wire-repr/examples/telegram.rs):**
  performs `req_pq_multi → resPQ` against DC2 with derived lengths, computed `message_id` and
  nonce, and typed TL views.
- **[Recursive expression VM](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.1/wire-repr/examples/expression_vm.rs):**
  compiles `(20 + 22) * -2` to recursive bytecode under a computed FNV-1a digest and evaluates it.

The network examples are plaintext demonstrations and do not authenticate the responder.

## Limits

`wire-repr` describes physical bytes. It is not a serialization framework: it does not build owned
object graphs, mutate views in place, reflect schemas at run time, or own `Read`/`AsyncRead`
transport. Protocol meaning stays in your code.

## Project

- [ARCHITECTURE.md](https://github.com/SilentBless/wire-repr-rs/blob/master/ARCHITECTURE.md):
  generated state, recursion, and measurement invariants.
- [ROADMAP.md](https://github.com/SilentBless/wire-repr-rs/blob/master/ROADMAP.md): ordered open work.
- [CONTRIBUTING.md](https://github.com/SilentBless/wire-repr-rs/blob/master/CONTRIBUTING.md):
  the checks CI runs and how to run them locally.
- [CHANGELOG.md](https://github.com/SilentBless/wire-repr-rs/blob/master/CHANGELOG.md).

## License

MIT © 2026 SilentBless. See the
[license](https://github.com/SilentBless/wire-repr-rs/blob/master/LICENSE).
