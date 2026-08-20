# Architecture

This document is normative.

## Package boundary

The repository root is a virtual workspace. It owns shared metadata,
documentation, licensing, and toolchain settings; it owns no Rust source or
tests. There are exactly two crates: `wire-repr/`, the public runtime facade,
and `wire-repr-macros/`, the procedural-macro compiler. There is no third
support crate.

`wire-repr/` owns wire-representation types and encoding/decoding behavior
exposed by its public API. It remains usable in
`no_std` environments without allocation. It depends on `wire-repr-macros`
only to reexport the public `wire_repr!` facade macro; the macro dependency
adds no target-runtime schema or machinery.

## Codec boundary

Fixed codecs own representations with a compile-time width and decode every
exact-width bit pattern. `bytes(N)` exposes an uninterpreted borrowed span.
Prefix codecs own representations whose encoded width must be discovered from
the input, so they perform bounded structural prefix parsing. Layout composition
owns field order and exact represented extents. Domain validation belongs to
consumers.

Encoding performs all fallible work before bytes are copied into caller-owned
output. A returned encoding error leaves the complete caller-owned output
unchanged. Successful plans must emit a representation that decodes to the planned
semantic value. Exact input encodings remain available
to layout views and are not reconstructed from decoded values.

An `as TypePath` mapping is a total nominal API adaptation, not a codec. Only built-in fixed
integers and `bytes(N)` are eligible. Its raw type is the physical codec type exactly
(`u32` for `U24`; `[u8; N]` for `bytes(N)`), and it uses direct `Semantic: From<Raw>` on
read and `Raw: From<Semantic>` on write. The physical codec retains byte ownership,
planning, range errors, and whole-destination atomicity; mappings add no fallible layer.
Mapped byte values are owned arrays or wrappers, while unmapped `bytes(N)` remains borrowed.
`scalar Name: Codec;` instead declares a reusable codec-owning nominal wrapper. Mappings do
not apply to declared scalar, custom/direct, prefix, or byte range fields.

## Layout macro boundary

Procedural-macro implementation belongs exclusively to `wire-repr-macros/`.
The compiler parses canonical layout syntax, normalizes and checks it before
rendering, preserves user documentation, and emits concrete operations. Normalization owns
mapping eligibility and the exact `MappingRaw` type; renderers consume that normalized fact
rather than rediscovering codec categories. The hidden `OwnedBytes` helper exists only as
macro support for mapped fixed bytes and is not a normal user API.
Sequential layouts have two exclusive modes. If no physical entry supplies
`position`, normalization infers contiguous one-based physical positions from physical
source declaration order across fields and anonymous padding/alignment entries. If any
entry supplies `position`, every physical entry must do so and supplied positions must be
contiguous; explicit mode may reorder physical placement independently of declaration order.
In both modes normalization lowers to concrete contiguous one-based physical order before
rendering, while declaration order owns generated API and documentation.

Padding consumes a fixed nonzero length. Alignment is relative to the represented layout
start and consumes the minimum bytes required by a nonzero boundary. Spacing bytes remain
opaque exact input; named reserved spans use `bytes(N)` and remain consumer-interpreted.
Sequential layouts may mix these entries with fixed codecs, custom prefix codecs, and byte
ranges. The complete byte-range algebra is:

- `bytes(current_pos..current_pos + source)`: relative payload length;
- `bytes(current_pos..source)`: exclusive absolute payload endpoint from representation
  byte zero;
- `bytes(current_pos..buf_end)`: supplied view-buffer tail.

The first two forms require an eligible physically preceding source: a built-in fixed
integer or semantic mapping over one. Framing uses its raw physical integer and checked
`usize` conversion; mappings do not change geometry. Prefix, custom/direct, declared
scalar, nominal, and byte-range sources are unsupported. `bytes(0)` is invalid; dynamic
ranges may be empty. A source may appear later in declaration order under explicit
placement, but must physically precede every framed range.

A `buf_end` range has no source, occurs at most once, and must be physically last. It owns
every byte after prior physical entries within the supplied input, including an empty span;
it establishes no external packet, FCS, transport, or other boundary. `with_remainder` returns
the suffix after the complete represented layout—not automatically after an absolute range
endpoint if later physical fields exist. Because `buf_end` is physically terminal, its suffix
is empty. Variable-width-source framing (for example, a ULEB128 WebAssembly section size)
remains consumer-owned.

Dynamic sequential views store represented bytes and exact exclusive prefix/range/buf_end
endpoints. They never cache semantic values or scan runtime metadata.
Exact prefix encodings remain directly available even when legal noncanonical forms.

Every absolute-layout field requires an explicit zero-based offset. Absolute
layouts check offsets in ascending offset order; their width is the maximum
field extent, gaps remain opaque represented bytes, and runtime codec-width
overlaps are rejected before input slicing.

The render dispatcher has separate fixed-sequential, dynamic-sequential, and
absolute owners. The layout declaration itself is the generated immutable view type;
`Layout::view` produces a lightweight request whose `with_remainder` or
`without_trailing` terminal validates exactly once. A generated view borrows only the accepted exact prefix and
exposes fixed getters without content validation. Prefix extents from custom
codecs are structurally parsed and checked before slicing. An
eligible unsigned builtin
storage field may own immutable bit projections. They own no bytes or parse
errors, use LSB0 numbering over the decoded semantic integer regardless of
endianness, and render as direct storage-getter shift/mask operations. Signed
and custom codecs have no projection contract; there is no runtime bit-storage
trait, metadata, or validation layer. Mappings do not alter physical storage: mapped integer
projections operate on the decoded raw integer. Mapped fields expose semantic and raw
getters; eligible mutable fields expose semantic and raw setters. Builder semantic and raw
inputs share one slot and the last input wins. A mapped byte-range source exposes both
getters but no setter or builder input; the builder derives its raw source value from the
byte range.

Generated mutable views expose immutable getters and only those typed
same-width setters that cannot change framing. They expose no unrestricted
mutable-byte access. Dynamic sequential views therefore omit setters for prefix
fields, byte ranges, `buf_end` ranges, and every fixed field used as a byte range source; each
byte range or `buf_end` range instead exposes a mutable slice of exactly its validated span, which
cannot resize or reframe it. View conversion preserves dynamic boundaries without
reparsing.

Generated builders retain values and encoding plans through complete preflight,
then commit physical fields in physical order. Missing values and codec planning follow
declaration order; all plans, extents, source conversions, checked arithmetic, shared-source
agreement, and output capacity precede mutation. Relative sources derive payload lengths.
Absolute sources derive physical exclusive payload ends, including preceding fixed and prefix
widths, padding, alignment, and ranges. `buf_end` has no source and its supplied slice
participates in aggregate extent. Pre-write-derived fixed fields have no ordinary builder input
or setter; their fallible derivations run during preflight in dependency order. Shared range
sources use identical algebra and values, and conversion/planning occur once.

Explicit contexts are borrowed builder-only inputs stored as `Option<&'value Referent>`,
including unsized referents. They are neither encoded bytes nor parser or view state, and a
missing context fails preflight before output mutation. Post-write finalizers target only direct,
unmapped, unprojected built-in fixed integers whose complete semantic domain is infallibly
encodable; `BeU24` and `LeU24` are excluded. Finalizer targets have no ordinary input or setter.
During physical commit, every finalizer target is written as zero; finalizers then run in stable
compile-time DAG order. Operands may borrow explicit contexts, semantic field values, or represented byte spans;
only a value dependency on another finalized field creates a finalizer-order edge. Each
finalizer returns the target's exact semantic type and immediately performs an infallible patch.
Its `buf_end` is the represented extent, and existing destination spans may be read but are not
rewritten. Thus every operation that can fail still completes before the first destination write.
Padding, alignment bytes, absolute-layout gaps, and bytes after the represented prefix remain
unchanged.

The macro emits no runtime descriptors, schema walkers, allocation, dynamic
dispatch, generated source files, or build scripts. Independently owned physical
bitfields, nested byte range schemas, and repeated sequences are excluded. Prefix,
byte range, padding, and alignment composition is sequential-only; absolute layouts
remain fixed-width.

## Constraints

- The target library uses only safe Rust; `unsafe` is forbidden.
- The target library has no runtime dependencies.
- Allocation is not a supported requirement of the target library.
- Default features remain empty; optional behavior requires an explicit feature
  and must preserve the baseline contract.
- Public behavior is deterministic from explicit inputs and does not depend on
  ambient runtime state.

## Ownership

Runtime codec contracts and built-ins live under `wire-repr/src/codec`. The
`wire-repr/src/lib.rs` crate root is the documented public facade and macro
reexport. Public cross-owner behavior is tested through integration tests in
`wire-repr/tests`; narrow implementation invariants remain with their owner.
