# Architecture

This document is normative. It defines the ownership boundaries and observable
contracts of `wire-repr`; implementation details may vary only when they preserve
these rules.

## 1. Package boundary

The repository root is a virtual workspace. It owns shared metadata, licensing,
toolchain policy, and documentation; it owns no Rust source or tests.

The current release line is 0.5.

The workspace has exactly two crates:

- `wire-repr/` is the public runtime facade. It owns public codec contracts,
  built-ins, and generated wire-representation behavior.
- `wire-repr-macros/` is the procedural-macro compiler. It owns layout syntax,
  normalization, validation, and rendering.

There is no target-runtime support crate. The facade reexports `wire_repr!`; that
host-side dependency must not become target-runtime schema machinery.

## 2. Physical representation versus domain ownership

`wire-repr` owns bytes, widths, offsets, framing, checked structural parsing,
encoding plans, and the represented extent. Consumers own domain meaning,
including magic values, reserved-byte policy, checksums as a protocol rule,
cross-field relationships, state machines, I/O, and allocation policy.

A fixed codec owns a compile-time-width representation and decodes every
exact-width bit pattern. `bytes(N)` is an uninterpreted physical span. A
self-delimiting codec owns bounded structural discovery of its encoded width. Layout
composition owns physical order and represented extent.

The framework must not turn domain rejection into structural parsing, nor use a
semantic value to reconstruct accepted raw bytes. Exact accepted self-delimiting
encodings, including legal noncanonical ones, remain available from immutable views.

## 3. Compile pipeline

The macro pipeline is:

1. Parse canonical declaration syntax and preserve user documentation.
2. Normalize declarations into physical geometry and public API facts.
3. Validate layout-specific structural constraints before rendering.
4. Render concrete, layout-specific public types and operations.

Normalization is the only owner that classifies mappings and determines each
mapping's exact raw type. Renderers consume normalized facts; they must not
rediscover codec categories independently.

The dispatcher has distinct owners for fixed sequential, dynamic sequential, and
fixed absolute layouts. A renderer may share deliberate utilities, but these three
geometry classes must not collapse into a runtime schema abstraction.

## 4. Generated public surface

For `pub layout Foo`, the declaration itself generates immutable `Foo<'wire>`.
`Foo::view(bytes)` returns a lightweight framing request. Its only parsing
terminals are:

- `with_remainder()`, which returns `(Foo<'wire>, &'wire [u8])`; and
- `without_trailing()`, which rejects a suffix.

A terminal performs structural validation exactly once. The immutable view borrows
only the accepted representation; `as_bytes()` excludes a `with_remainder` suffix.
Fixed getters decode their validated field bytes without domain validation.

Generated mutable types retain the `FooViewMut` naming family. They parse with
`parse_prefix_mut` or `parse_exact_mut`, rather than through `Foo::view`. Builders
retain the `FooBuilder` naming family. Error types distinguish their operation and
phase; successful public operations must not expose a runtime schema.

## 5. Sequential physical geometry

Sequential layouts have two exclusive placement modes.

In implicit mode, no physical entry uses `@ N`; fields, padding, and alignment
receive contiguous one-based physical positions in source order. In explicit mode,
every physical entry uses `@ N`; positions must be contiguous, but physical order may
differ from declaration order.

Declaration order governs public getter, setter, builder-input, error-selection,
and rustdoc order. Physical order governs parsing, represented bytes, dynamic
progress, commit order, and physical layout errors. A source may therefore be
later in declaration order but must be physically earlier than every range it
frames.

Padding consumes a fixed nonzero length. Alignment is relative to representation
byte zero and consumes the minimum bytes for a nonzero boundary. Both are opaque
represented bytes. Named reserved bytes are ordinary `bytes(N)` and remain
consumer-interpreted.

## 6. Fixed absolute geometry

Every absolute-layout field has a mandatory zero-based `@ N` placement. Absolute
layouts are fixed width: their width is the maximum field extent. Parsing checks
fields in ascending offset order and rejects invalid width, overflowing extent, and
overlap before slicing input.

Gaps are part of the represented byte span. Views preserve them; builders preserve
existing caller-output gap bytes. Absolute layouts never infer offsets and do not
support padding, alignment, self-delimiting codecs, or dynamic byte ranges.

## 7. Immutable parsing and framing

Structural parsing advances in physical order. It validates codec configuration,
checks bounds and arithmetic, determines dynamic extents, and only then forms
field slices. It must not blindly slice after an unchecked codec claim.

For fixed sequential and absolute layouts, represented extent follows fixed
geometry. For dynamic sequential layouts, the immutable view retains the exact validated
`usize` endpoint for every self-delimiting field, range, and terminal-buffer boundary.
It uses those endpoints directly: getters do not re-scan bytes, recompute endpoints,
cache semantic values, or defer framing checks.

`with_remainder` returns the suffix after the complete represented layout. It does
not stop at an intermediate absolute range endpoint if later physical entries
exist. `without_trailing` requires that suffix to be empty.

Prefix validation receives the remaining input, returns a nonzero encoded extent,
and is checked against it. Decoding receives exactly that accepted prefix span.
The raw getter returns those exact bytes; it does not reencode the decoded value.

## 8. Dynamic range algebra

Only sequential layouts may contain byte ranges:

- `bytes(source)` uses `source` as a relative length.
- `bytes_to(source)` uses `source` as an exclusive endpoint relative to
  representation byte zero.
- `remaining_bytes` consumes the supplied view-buffer tail.

The first two forms require an eligible physically preceding built-in fixed integer
actually consumed by at least one range. An unannotated source uses its raw physical
integer with checked `TryInto<usize>` parsing and `TryFrom<usize>` preparation; a
total `as Semantic` mapping remains distinct, affects nominal getters only, and does
not change that geometry. A source annotated with `range_source: Adapter` instead
uses `RangeSource<Codec>` for checked bidirectional structural conversion. Parsing
calls `to_bytes(raw source)` at the consuming range before the existing checked
bounds/range logic. `prepare()` derives required byte geometry, requires shared
sources to agree on that geometry, calls `from_bytes` once per source, then plans its
physical codec. This canonicalizes the encoded source from requested geometry; no API
preserves contradictory or noncanonical source values. Commit does neither conversion
nor planning and remains capacity-only/atomic. Failed adapter conversion is reported by
concrete generated source-local parse or write error variants.

Adapters cannot coexist with a mapping, `derive`, or `finalize`. Unsigned bit
projections may coexist and read the same whole packed integer consumed and returned by
the adapter. Custom codecs, declared scalars, fixed bytes, prefix codecs, ranges,
absolute layouts, and `remaining_bytes` are unsupported sources. Built-in-integer-only
adapter support is a hard ownership boundary: borrowed custom `FixedCodec` values or
plans can require self-referential prepared storage. The raw source getter remains the
wire integer; an adapter does not create a geometry getter. Checked structural
underflow, alignment, and encoded-field bounds belong to the adapter; unrelated
protocol/RFC policy remains consumer-owned.
`bytes(0)` is invalid, but a dynamic range may be empty.

An absolute endpoint before the current physical endpoint is a range error. An
endpoint beyond available input is an input-shortage error. These are distinct
conditions.

`remaining_bytes` has no source, appears at most once, and is physically last. It
owns all bytes after earlier physical entries in the supplied input, including an
empty span. Its `with_remainder` suffix is therefore empty. It does not establish an
external packet, FCS, transport, or application boundary.

Self-delimiting values are not range sources. Consumer code owns framing such as a
ULEB128 section length.

## 9. Mutable contract

A mutable view has the same represented extent and dynamic boundaries as an
immutable view accepted from the same bytes. Converting its view form must retain
those boundaries without reparsing.

Mutable views expose immutable getters and only typed, same-width setters whose
writes cannot invalidate framing. They expose no unrestricted mutable-byte API.
Self-delimiting codecs, dynamic ranges, `remaining_bytes`, and every range source
have no setter.
Each range instead exposes a mutable slice of its exact validated span; that slice
cannot resize or reframe the representation.

Mapped eligible fields expose semantic and raw getters/setters. A mapping does not
change physical encoding or range-source behavior. Projections remain immutable
views over their storage getter; they do not independently own bytes or setters.

## 10. Builder phases and atomicity

A builder has a strict two-phase contract.

**Preflight** completes all fallible work before any caller-output mutation:
required inputs and contexts, codec planning, plan-length validation, derivations,
range geometry conversion, shared-source agreement, dynamic geometry, checked
arithmetic, and output capacity. Missing inputs and planning follow declaration
order. For every source, preflight derives geometry from the consuming ranges,
requires all shared consumers to agree on it, converts it to the source's encoded
value once, and plans the physical codec before writing dependent output.

**Commit** writes physical entries in physical order. It returns the mutable
representation plus a disjoint untouched suffix. Padding, alignment, absolute
gaps, and bytes after the represented extent retain their existing output contents
unless a range explicitly writes them.

Any returned build error leaves the complete caller-provided output slice
unchanged. This includes a short output, codec planning failure, invalid plan
length, failed derivation, conversion failure, arithmetic overflow, and
shared-source conflict.

Relative range sources derive payload lengths. Absolute range sources derive
exclusive physical payload ends, including preceding fixed/self-delimiting extents,
padding, alignment, and earlier ranges. `remaining_bytes` has no source; its supplied
builder span participates in the represented extent.

## 11. Codec laws

`FixedCodec::WIDTH` is nonzero and its successful plan has exactly that width.
`PrefixCodec::validate_prefix` reports a nonzero extent within its supplied bytes; a
successful self-delimiting plan is nonempty. `EncodePlan::encoded_len` and `write_into`
refer to the same representation. A `RangeSource` must coherently round-trip each
supported fixed source value and byte geometry, use checked arithmetic, and report an
explicit conversion error; it establishes structural validity, not protocol policy.

A successful plan must encode the planned semantic value. Any violation is
reported before output mutation where the generated operation can observe it.
Codec contracts own representation errors, not application-level semantic policy.

## 12. Mappings, projections, derivations, and finalization

`as TypePath` is a total nominal mapping, not a codec. It applies only to built-in
fixed integers and `bytes(N)`, with `Semantic: From<Raw>` and `Raw: From<Semantic>`.
Its raw type is the exact physical type (`u32` for `U24`, `[u8; N]` for bytes).
Mapped byte values are owned; unmapped fixed bytes are borrowed.

Mapped fields expose semantic and raw forms. Semantic and raw builder inputs share
one slot; the last input wins. A range-source mapping exposes both getters, but no
ordinary setter or builder input because the source is derived.

`scalar Name: Codec;` declares a reusable codec-owning nominal wrapper; it is not
an `as` mapping. Mappings do not apply to declared scalars, custom/direct codecs,
self-delimiting codecs, or ranges.

Unsigned built-in storage may declare immutable LSB0 bit projections. Bit numbering
is over the decoded raw integer regardless of endianness. Mapped integer
projections likewise use raw storage. Signed and custom storage have no projection
contract.

A `derive` field runs fallibly in preflight dependency order and has no ordinary
input or setter. A builder-only `context` is stored as an explicit borrowed
`Option<&Referent>`, including unsized referents; it is neither encoded bytes nor
parser/view state.

A `finalize` target is a direct, unmapped, unprojected built-in fixed integer whose
complete semantic domain is infallibly encodable; `BeU24` and `LeU24` are excluded.
It has no ordinary input or setter. Commit writes its field as zero, then finalizes
in stable compile-time DAG order. Only value dependencies between finalizers make
DAG edges; byte-span reads do not.

Finalizers return the target's exact semantic type and patch only that target
infallibly. They may read contexts, semantic values, represented spans, and
existing destination spans, but may not rewrite arbitrary spans. Their represented
end is the representation extent, not caller-output length. Thus no operation that
can fail remains after the first output write.

## 13. Error phases

Errors preserve structural phase and physical/declaration ownership:
configuration/geometry errors precede input access in physical order; shortage and
malformed prefix/range structure arise during parsing; trailing bytes arise only at
exact framing; missing inputs and planning failures follow declaration order; range
conflict, conversion, extent, and capacity failures arise in preflight; and mutation
errors arise before their individual setter writes.

Error reporting must not conceal a later failure by performing unchecked work
first, and no error path may mutate caller output before preflight completes.

## 14. Zero-cost and codegen contract

The macro emits concrete operations, not runtime descriptors, reflection, schema
walkers, erased codecs, dynamic dispatch, allocation, generated source files, or build
scripts. Fixed access is ordinary safe Rust loads, conversions, shifts, masks, and
bounded copies; dynamic access uses retained validated endpoints.

Assembly text is not a stable API: instruction selection and register allocation
are compiler- and target-specific. The release probes in `wire-repr/tests/codegen.rs`
are illustrative comparisons with handwritten safe Rust. `ci/check-codegen.py` is
the normative release gate for the expected optimized operation shape and absence
of framework machinery.

## 15. Explicit exclusions

The architecture excludes runtime schemas, endpoint recomputation, repeated
sequences, arbitrary conditional fields, nested range schemas, independently owned
bitfields, parser-owned protocol semantics, and target-runtime dependencies. Each
layout declaration has one immutable owner; alternate compatibility owners are not
generated. These are exclusions, not deferred subsystems.

## 16. Testing and ownership

Runtime codec contracts and built-ins live under `wire-repr/src/codec`; the facade
and macro reexport live in `wire-repr/src/lib.rs`; parsing/rendering belongs to
`wire-repr-macros`. Public cross-owner behavior belongs in `wire-repr/tests`.
Narrow owner invariants remain with their owner. Tests must exercise contracts at
the relevant boundary rather than duplicate the production algorithm as an oracle.

The baseline target library is `no_std`, safe Rust only, has no target-runtime
dependencies, uses empty default features, and is governed by Rust 1.91 / edition
2024 with `unsafe_code = "deny"`.