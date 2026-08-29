<h1 align="center">wire-repr</h1>

<p align="center">
  <strong>Exact-source wire views and progressive writers from ordinary Rust schemas.</strong>
</p>

<p align="center"><code>no_std</code> · no allocation · safe generated API · Rust 1.91</p>

`wire-repr` is for binary formats whose bytes remain the source of truth: network packets,
file records, storage pages, firmware messages, and IPC frames. Describe the physical layout once;
the derives generate an immutable view and an output-owning typestate writer.

> [!IMPORTANT]
> A schema is a description of physical bytes, not a decoded semantic object. `wire-repr` owns
> widths, byte order, framing, selectors, controllers, and exact ranges. Your application keeps
> ownership of protocol meaning.

---

## ✨ Why wire-repr

- **Exact-source views.** Read fields without materializing a second object graph or losing the
  represented bytes.
- **Progressive writers.** Emit directly into caller-owned fixed, growable, bounded, or custom
  output.
- **Static composition.** Generic children, enums, arrays, and computed fields remain monomorphized
  Rust.
- **No hidden storage.** The core does not allocate, build collection indexes, or dispatch through
  runtime schemas.
- **Fail-closed layouts.** Ambiguous or unsupported declarations fail during derive expansion
  instead of selecting a plausible runtime interpretation.

## 🚀 Quick start

```toml
[dependencies]
wire-repr = "1"
thiserror = { version = "2", default-features = false }
```

`thiserror` supplies the nominal generated error derives; schema crates declare it directly. Both
dependencies are `no_std`. `wire-repr` itself has no Cargo features.

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

`payload_len` remains a real physical field. Reading trusts it; writing derives and patches it from
`payload`, so no length setter exists. Constants follow the same rule: validate on read, emit
automatically on write.

## 🧭 Views and ownership

`Schema::view<T: AsRef<[u8]>>(input: T)` stores `T` directly:

```rust
let borrowed = Packet::view(&bytes[..])?;
let owned = Packet::view(bytes.to_vec())?;
let shared = Packet::view(bytes::Bytes::copy_from_slice(bytes))?;
```

Passing a reference borrows. Passing a collection or shared handle moves it into the opaque view.
Nested getters borrow their parent and reconstruct only ranges already proved by framing. There is
no separate owned renderer or schema lifetime.

> [!NOTE]
> Retained backing must keep projecting the same immutable byte span while the view exists. Slices,
> arrays, `Vec`, `bytes::Bytes`, and ordinary wrappers satisfy this. Intentionally stateful
> `AsRef<[u8]>` implementations do not.

## ✍️ Progressive output

`Schema::builder(output)` owns one cursor. Every setter consumes one typestate stage and returns the
next, so `?` is the normal control flow and `finish()` exists only after all required fields are
present.

```rust
Packet::builder(&mut fixed_slice[..]);      // fixed; returns NeedMore
Packet::builder(&mut vec);                  // grows through Extend<u8>
Packet::builder(output::bounded(&mut vec, limit));
Packet::builder(output::grow_with(&mut arena, grow));
Packet::builder(output::owned(vec));        // movable through a 'static worker
```

Generated code keeps offsets rather than pointers, so growable storage may relocate safely.
`finish()` returns `Written<O>` with the exact represented range. `output::owned` returns the
caller-selected collection without adding an allocator dependency to the core.

> [!WARNING]
> Writing is progressive, not transactional. An error may leave partial unpublished bytes. Publish
> only the `Written` result, or use an unpublished slot/double buffer when the surrounding system
> requires atomic visibility.

## 🌐 Real network examples

These examples send real packets and parse the returned bytes:

```text
cargo run -p wire-repr --example dns -- example.com
cargo run -p wire-repr --example ntp
cargo run -p wire-repr --example telegram
cargo run -p wire-repr --example expression_vm
```

- **[DNS over UDP](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.1/wire-repr/examples/dns.rs).**
  Builds a query, uses the system resolver, frames the response, and prints flags and section
  counts.
- **[NTPv4 over UDP](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.1/wire-repr/examples/ntp.rs).**
  Builds a client packet, validates server timestamps and synchronization state, unfolds NTP eras,
  and calculates the observed clock offset.
- **[Telegram MTProto over TCP](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.1/wire-repr/examples/telegram.rs).**
  Connects to DC2 and performs `req_pq_multi → resPQ`. The transport marker is a constant, both
  lengths are derived, `message_id` and `nonce:int128` are computed, and the response uses typed TL
  views plus schema validators and a runtime fingerprint array.
- **[Recursive expression VM](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.1/wire-repr/examples/expression_vm.rs).**
  Compiles `(20 + 22) * -2`, wraps the recursive bytecode in a computed FNV-1a digest, validates it,
  and evaluates retained child views.

> [!NOTE]
> DNS and NTP are plaintext demonstrations. Correlation fields do not authenticate a responder,
> and the Telegram bootstrap stops before the key exchange proves server identity.

The network binaries use `std` plus the dev-only `getrandom` dependency. Neither enters the normal
`wire-repr` target graph. Reads have explicit deadlines or datagram limits, and each endpoint can be
overridden from the command line.

## 🧩 Layout toolbox

- **Scalars and arrays:** fixed integers and floats, explicit endian, constants, `[u8; N]`, and
  fixed primitive arrays.
- **Logical types:** `as = Physical` with checked bidirectional `TryFrom`, including `NonZero*` and
  user newtypes.
- **Geometry:** bounded `wire::Bytes`, terminal `rest`, padding, alignment, and forward placement.
- **Dependencies:** shared length controllers, conditional groups, flags, inline bit projections,
  and counted runtime arrays.
- **Static enums:** selector-only unit variants, body variants, and exact unknown-body forwarding.
- **Physical selections:** ordered root/nested byte ranges for copying, hashes, and checksums.
- **Computed fields:** infallible or fallible callbacks scheduled by a generated dependency DAG.
- **Sequences:** homogeneous `views()` and failure-atomic heterogeneous cursors.

A runtime array keeps its count as an ordinary physical field:

```rust
#[derive(WireView, WireBuilder)]
struct Items<T> {
    #[wire(le)]
    count: u16,
    #[wire(counted_by = count)]
    values: wire::Array<T>,
}
```

`ArrayView` retains a range and authoritative count—not item offsets. Iteration advances one cursor;
writers accept arbitrary `IntoIterator` sources through `try_extend`. A validated source array can
be forwarded as one exact range:

```rust
packet.values(|values| values.copy_from(source.values()))?;
```

Computed callbacks may consume logical getters and fragmented physical selections:

```rust
#[wire(be, computed = checksum(exclude(self)))]
checksum: u16,
```

Callbacks with arguments use the generated view to resolve exact fields. Zero-argument callbacks
specialize to direct calls and remain available to `WireBuilder`-only schemas.

## 🌀 Recursive layouts

Closed selector enums may recurse through counted arrays or object fields marked with
`wire::Recursive<T>`:

```rust
let value = Value::view::<64>(input)?;
```

The const parameter bounds one iterative continuation stack. Recursive arrays retain compact exact
geometry rather than item offsets; unsupported shapes fall back to exact replay. Progressive
writers stream children through one output cursor without retaining a semantic tree or encoded
plan.

## 🔬 What reaches the CPU

The repository compares every shipped layout class against independent idiomatic handwritten and
best-safe implementations. A pinned Rizin backend inspects final ELF, Mach-O, and PE consumers,
separating named linker calls from unresolved dispatch before runtime samples are interleaved.

> [!NOTE]
> LLVM instruction counts are diagnostic evidence, not a latency oracle. Workload-local formulas
> own hard failures and optimization attention.
>
> Repository contributors running the measurement harness need Rizin 0.9.1 on `PATH`; CI installs
> the release package by checksum. `WIRE_REPR_RIZIN` may point to another executable location.

The core remains featureless and allocation-free; optional application storage appears only through
caller-selected input/output types. See [`ARCHITECTURE.md`](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.1/ARCHITECTURE.md)
for the generated-state, recursion, and measurement invariants.

## ⚠️ Errors and incomplete input

Generated errors are nominal field-site enums. Nested failures keep their concrete source type and
read errors carry absolute root-input offsets. Incomplete contiguous input returns:

```rust
NeedMore {
    offset,
    additional_at_least,
}
```

The caller owns buffering and retries. `wire-repr` does not own `Read`, `AsyncRead`, segmented
input, or resumable transport state.

## 🎯 Deliberate limits

> [!NOTE]
> `wire-repr` is a physical-representation compiler, not a serialization framework or protocol
> runtime.

The `1.0` core does not provide mutable views, semantic object materialization, runtime schema
reflection, negotiated selector registries, hidden collection indexes, async transport I/O, general
resource-limit machinery, or general traversal.

For the full API, manual capability contracts, and schema reference, see the
[crate documentation](https://docs.rs/wire-repr).

## 🛠️ Verification

```text
cargo +1.91.0 test --workspace --all-targets
cargo +1.91.0 clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo +1.91.0 doc --workspace --no-deps
python3 ci/check-fail-fast.py
cargo +1.91.0 run -p wire-repr-measure --release -- run
```

Coverage-guided fuzz targets live under `fuzz/` for compound framing, progressive round trips,
recursive access, and failure-atomic sequences/cursors. For a local smoke run:

```text
cargo +nightly-2026-08-01 fuzz run recursive -- -max_total_time=20 -max_len=512
```

Pull requests run all four targets for 20 seconds each. The scheduled workflow extends each target
to 120 seconds without adding that cost to merge commits.

## 📄 License

MIT © 2026 SilentBless. See the
[license](https://github.com/SilentBless/wire-repr-rs/blob/v1.0.1/LICENSE).
