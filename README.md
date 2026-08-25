<h1 align="center">wire-repr</h1>

<p align="center"><strong>Compile Rust wire schemas into zero-copy views and progressive writers.</strong></p>

<p align="center"><code>no_std</code> · no allocation · safe public API · Rust 1.91</p>

`wire-repr` treats a Rust schema declaration as a physical representation, not as the decoded
value. Reading returns an opaque exact-source view over the caller's backing. Writing uses a
typestate writer over caller-owned fixed, growable, bounded, or custom output.

The implemented production surface is the featureless `WireView`/`WireBuilder` capability model.
[`ARCHITECTURE.md`](ARCHITECTURE.md) defines the complete layout contract.

## Add it

```toml
[dependencies]
wire-repr = "1"
```

The crate is featureless. Applications may pass `bytes::Bytes`, `Vec<u8>`, slices, or custom
`AsRef<[u8]>` backing without enabling a wire-repr feature.

## Generic exact-source view and progressive writer

```rust
use wire_repr::{WireBuilder, WireView};

#[derive(WireView, WireBuilder)]
struct HelpGetConfig {
    #[wire(le, constant = 0xc4f9_186b)]
    constructor: u32,
}

#[derive(WireView, WireBuilder)]
struct InvokeWithLayer<T> {
    #[wire(le, constant = 0xda9b_0d0d)]
    constructor: u32,
    #[wire(le)]
    layer: i32,
    query: T,
}

type Query = InvokeWithLayer<HelpGetConfig>;

let input = [
    0x0d, 0x0d, 0x9b, 0xda, // invokeWithLayer
    0xc8, 0x00, 0x00, 0x00, // layer 200
    0x6b, 0x18, 0xf9, 0xc4, // help.getConfig
];
let view = Query::view(input).unwrap();
assert_eq!(view.layer(), 200);
assert_eq!(view.query().constructor(), 0xc4f9_186b);
assert_eq!(view.as_bytes(), &input);

let mut output = [0xa5; 16];
let written = Query::builder(&mut output[..])
    .layer(200)?
    .query(|query| query)?
    .finish()?;
assert_eq!(written.range(), 0..12);
assert_eq!(written.as_bytes(), &input);
assert_eq!(&output[12..], &[0xa5; 4]);
```

`view(input)` stores `input` directly. Passing a reference creates a borrowed view; passing an
owned container moves it into the hidden view state. Nested getters borrow their parent and use
retained reference-free geometry state rather than reparsing.

Retained backing must keep projecting the same immutable byte span while the view exists. Slices,
`Vec`, `bytes::Bytes`, and ordinary owned wrappers satisfy this. Stateful `AsRef` implementations
that switch or mutate their projection are not supported.

Constants are validated on read and have getters, but no writer setters. Derived and manual
children use the same closure setter through public `WireBuilder` and `WireWrite<V>` capabilities.
Setters write progressively; only offsets and typestate remain in the generated writer.

## Manual wire types

Manual representations implement the same independent read and write capabilities as derived
schemas.

```rust
use wire_repr::{ChildWriter, Output, WireBuilder, WireWrite, WriteError};

struct LittleEndianWord;

impl WireBuilder for LittleEndianWord {
    const FIXED_SIZE: Option<usize> = Some(4);

    type Builder = ();

    fn builder() -> Self::Builder {}
}

impl WireWrite<u32> for LittleEndianWord {
    type Error = core::convert::Infallible;

    fn write<O: Output>(
        value: u32,
        writer: &mut ChildWriter<'_, O>,
    ) -> Result<(), WriteError<Self::Error, O::GrowError>> {
        writer.write(&value.to_le_bytes())?;
        Ok(())
    }
}
```

Manual writers receive the same progressive cursor as generated children. `FIXED_SIZE` enables a
manual child before later physical fields; omitting it keeps the manual representation
variable-width and terminal-only until dynamic geometry is available. Manual writers may return
semantic errors after partially modifying unpublished output; wire-repr never allocates, rolls
back, or clears bytes.

Manual `WireView` implementations are an explicit unsafe boundary: retained state must remain
memory-safe for any immutable span of the framed length. Generated APIs remain safe, retain
validated logical values when needed, and check manual child extents before reconstruction.

## Scalar representations

The schema model handles `u8`/`i8` and every 16-, 32-, 64-, and 128-bit integer in both byte
orders, plus `f32` and `f64`. One-byte fields have no endian attribute; every multibyte field
requires `le` or `be`.

Platform and logical Rust types declare their physical width explicitly:

```rust
#[derive(WireView, WireBuilder)]
struct Index {
    #[wire(as = u32, le)]
    offset: usize,
    #[wire(as = i64, be)]
    delta: isize,
    #[wire(as = u8)]
    enabled: bool,
    #[wire(as = u32, le)]
    character: char,
}
```

Read and write conversions are checked. Invalid stored values and values that do not fit their
declared wire width produce nominal field-site errors rather than truncating.

## Behavioral guarantees

- `view()` accepts exactly one representation and rejects trailing input.
- Errors identify the field site and absolute root-input offset.
- `NeedMore` reports a proven lower bound for incomplete contiguous input.
- Derived descriptors contain no input references or self-references; manual descriptors certify
  the same invariant through the unsafe `WireView` contract.
- Ordinary scalar getters remain ordinary scalar values.
- Generic and nested composition is static and monomorphized.
- `builder(&mut [u8])` writes into fixed output and returns `OutputError::NeedMore` when it ends.
- `builder(&mut Vec<u8>)` and `builder(&mut bytes::BytesMut)` grow automatically through
  `AsRef<[u8]> + AsMut<[u8]> + Extend<u8>`.
- `output::bounded` and `output::grow_with` opt into bounded or caller-controlled growth.
- On write failure, output may contain a partial unpublished representation. `finish()` returns a
  `Written` token with the exact represented range.
- Writers and views do not allocate or dispatch dynamically inside wire-repr.
- The public read API is identical for borrowed and retained-owned backing.

## Verification

The repository checks behavior, final linked artifacts, and runtime performance independently:

```text
cargo +1.91.0 test --workspace --all-targets
cargo +1.91.0 clippy --workspace --all-targets -- -D warnings
python3 ci/check-fail-fast.py
cargo +1.91.0 run -p wire-repr-measure --release -- run
```

The Rust measurement tool discovers capability-owned workloads below `wire-repr/measure`. Each
workload supplies generated, idiomatic, and best-safe implementations plus optional lower bounds.
Its own formulas decide hard failures, optimization attention, and additional derived metrics.
Human-readable output is the default; CI uses `run --json`. Artifact analysis reads final linked
symbols, while interleaved calibrated samples report median, p95, range, and median absolute
deviation instead of treating LLVM instruction counts as performance truth.

## Design direction

The implemented surface currently covers fixed scalar and byte-array structs, constants, explicit
logical conversions, schema validators, multiple fixed generic or manual children, one optional
terminal variable child, and progressive typestate writers.

The remaining production classes are variable nested fields,
raw bytes and rest spans, controllers and conditional groups, static selectors with exact unknown
forwarding, nominal and inline bitfields, padding/alignment/placement, demand-framed recursive
layouts, physical selections, computed fields, and capability-gated `views`/`cursor` traversal.
Collections retain only range and count; untouched nested values are not eagerly traversed.
Negotiated selector maps, hidden indexes, full-tree validation, and a general limits framework are
not part of the target core. Every new class must extend the progressive writer model and add
behavioral plus generated/idiomatic/best-safe workload evidence.
