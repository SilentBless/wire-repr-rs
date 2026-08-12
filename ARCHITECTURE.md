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
not apply to declared scalar, custom/direct, prefix, or region fields.

## Layout macro boundary

Procedural-macro implementation belongs exclusively to `wire-repr-macros/`.
The compiler parses canonical layout syntax, normalizes and checks it before
rendering, preserves user documentation, and emits concrete operations. Normalization owns
mapping eligibility and the exact `MappingRaw` type; renderers consume that normalized fact
rather than rediscovering codec categories. The hidden `OwnedBytes` helper exists only as
macro support for mapped fixed bytes and is not a normal user API.
Sequential layouts have two exclusive modes. If no physical entry supplies
`position`, normalization infers contiguous one-based physical positions from
physical source declaration order across fields and anonymous padding/alignment
entries. If any entry supplies `position`, every physical entry must do so and
the supplied positions must be contiguous; explicit mode may reorder physical
placement independently of declaration order. In both modes normalization lowers
to concrete contiguous one-based physical order before rendering, while
preserving declaration order as the owner of generated API and documentation.
Padding consumes a fixed nonzero length. Alignment is relative to the
represented layout start and consumes the minimum bytes required by a nonzero
boundary. Spacing bytes remain opaque exact input; named reserved spans use
`bytes(N)` and remain consumer-interpreted. Sequential layouts may mix these
entries with fixed codecs, custom prefix codecs, named opaque regions, and one
terminal opaque remainder. A region is framed by the checked `usize` conversion of a
physically preceding non-region field's decoded value. The source is decoded from its
exact accepted wire span before region capacity is checked; this framing use is the only
parse-time decode exception for prefix fields. Regions may be empty and expose only
their exact borrowed bytes. A remainder is sequential-only, may occur at most once, and
must be physically last. It owns every byte after prior physical entries within the
caller-supplied input and exposes that exact opaque borrowed span, including an empty
span. It differs from `region(length)`: it has no length source and does not establish,
claim, or infer an external packet, FCS, transport, or other framing boundary.
Dynamic sequential views store their represented bytes and one exclusive end
boundary per prefix field or bounded region; they never cache semantic values or scan
runtime metadata. A terminal remainder ends at the represented byte-slice boundary, so
it needs no duplicate stored boundary. Exact prefix encodings remain directly available
even when they are legal noncanonical forms.
Every absolute-layout field requires an explicit zero-based offset. Absolute
layouts check offsets in ascending offset order; their width is the maximum
field extent, gaps remain opaque represented bytes, and runtime codec-width
overlaps are rejected before input slicing.

The render dispatcher has separate fixed-sequential, dynamic-sequential, and
absolute owners. A generated view borrows only the accepted exact prefix and
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
inputs share one slot and the last input wins. A mapped region-length source exposes both
getters but no setter or builder input; the builder derives its raw source value from the
region.

Generated mutable views expose immutable getters and only those typed
same-width setters that cannot change framing. They expose no unrestricted
mutable-byte access. Dynamic sequential views therefore omit setters for prefix
fields, regions, remainders, and every fixed field used as a region length source; each
region or remainder instead exposes a mutable slice of exactly its validated span, which
cannot resize or reframe it. View conversion preserves dynamic boundaries without
reparsing.

Generated builders retain values and encoding plans through complete preflight,
then commit physical fields in physical order. Missing values and codec planning
follow declaration order; width and extent legality, shared-source agreement,
checked arithmetic, and output capacity all precede mutation. Dynamic builders
derive region length-source values from caller-provided region slices and accept
caller-provided remainder slices directly; a remainder length participates in checked
aggregate extent before its bytes are copied. Shared sources require equal region lengths,
and source conversion and planning happen once. Padding, alignment bytes,
absolute-layout gaps, and bytes after the represented prefix remain unchanged.

The macro emits no runtime descriptors, schema walkers, allocation, dynamic
dispatch, generated source files, or build scripts. Independently owned physical
bitfields, nested region schemas, and repeated sequences are excluded. Prefix,
region, padding, and alignment composition is sequential-only; absolute layouts
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
