# Changelog

All notable public changes to `wire-repr` are recorded here.

## [Unreleased]

## [1.2.0] - 2026-10-13

- Compose unbounded variable-width children before later fields when their read capability
  proves a leading extent. Parents retain validated child state and exact boundaries without
  reframing getters. Progressive and detached writers use each child's actual emitted length,
  including write-only manual children, and preserve following controller and computed patches.

## [1.1.1] - 2026-10-12

- Allow signed `counted_by` controllers across native `i8`, `i16`, `i32`, `i64`, and `i128`, plus
  logical `isize` with an explicit signed physical representation. Readers reject negative counts
  before array geometry, including through `view_unchecked()`, with no null sentinel. Progressive
  and detached writers derive counts and check both physical and logical representability.
  Fixed recursive arrays retain their compact `u32` continuation and existing count boundary;
  SLEB128 and ZigZag ULEB128/VU128 counts are supported only outside recursive body grammars.
  Byte-length, placement, and bit-projection controllers remain unsigned.
- Advance runtime-array iterators through their remaining input and omit unused absolute-start
  calculations for sequential raw fields, preserving exact item ranges and typed errors.
- Measure linked roles with ordinary release dead-code elimination. Exported entries remain
  referenced by the role CLI without forcing dead-code retention that changes hot-path optimization.
  Compound collection decoders share an inline byte-consumption boundary across all three roles.

## [1.1.0] - 2026-10-11

- Add ULEB128, SLEB128, and VU128 integer field encodings, with ZigZag available alongside
  ULEB128 or VU128 for signed physical integers. Checked conversions and constants compose
  with the encodings; views preserve legal nonminimal source bytes.
- Support unsigned variable-integer byte/count controllers in detached and progressive writers.
  Deferred prefixes expand in the caller's output before computed fields are patched, without
  hidden allocation or a larger-than-final fixed buffer.
- Bound transitive artifact analysis by linked function extents, preventing non-returning panic
  branches from attributing a neighboring function's allocations and dispatch to the workload.
- Recognize named linker calls cached in x86-64 callee-saved registers without misclassifying them
  as runtime dispatch; unknown targets and lost provenance remain unresolved.

## [1.0.4] - 2026-10-11

- Support counted arrays in detached nested struct and enum-body builders through
  `extend(source, callback)`. Sources stream once during writing without retaining item plans;
  count fields are patched at their physical offsets, including after dynamic data.

## [1.0.3] - 2026-09-24

- Delimit nested variable-width counted arrays before following fields, including zero-count
  arrays, without eagerly validating terminal arrays in exact views.

## [1.0.2] - 2026-09-24

- Allow counted arrays whose physical count field follows dynamic byte geometry.
  Progressive writers patch the count at its computed position; terminal arrays
  retain their existing deferred item validation.

## [1.0.1] - 2026-08-29

- Documented the direct `thiserror` dependency used by nominal generated error types.

## [1.0.0] - 2026-08-29

The first stable release of `wire-repr`.

### Views

- Independent `WireView` derive capability over arbitrary retained `AsRef<[u8]>` backing.
- Exact framing, field-site errors with absolute offsets, borrowed nested views, and lazy scalar
  decoding.
- Homogeneous sequence views, failure-atomic heterogeneous cursors, and exact View forwarding.
- Depth-bounded recursive enum arrays and object continuations with compact retained geometry.

### Writers

- Independent `WireBuilder` derive capability with progressive output-owning typestate writers.
- Fixed, growable, bounded, callback-driven, and owned output targets.
- Streaming runtime arrays, arbitrary `IntoIterator` sources, and bulk exact collection forwarding.
- Progressive recursive object and array writers with no retained semantic tree or encoded plan.

### Layouts

- Fixed scalars and primitive arrays, constants, checked logical conversions, validators, and
  generic or manual nested children.
- Dynamic byte geometry, padding, alignment, placement, shared controllers, conditional groups,
  counted arrays, static enums, and nominal or inline bitfields.
- Nested physical selections and dependency-ordered computed fields.

### Guarantees

- `no_std`, no allocation inside wire-repr, no runtime dispatch, and no Cargo features.
- Generated, idiomatic handwritten, and best-safe comparison workloads for every shipped layout
  class.
- Behavioral, compile-failure, protocol, coverage-guided fuzzing, documentation, package, MSRV,
  and cross-target release checks.
- Executable DNS, NTP, Telegram MTProto, and recursive expression-VM showcases exercise real
  network bytes, computed fields, schema validators, runtime collections, and recursive writers.

[Unreleased]: https://github.com/SilentBless/wire-repr-rs/compare/v1.2.0...master
[1.0.0]: https://github.com/SilentBless/wire-repr-rs/releases/tag/v1.0.0
[1.0.1]: https://github.com/SilentBless/wire-repr-rs/releases/tag/v1.0.1
[1.0.2]: https://github.com/SilentBless/wire-repr-rs/releases/tag/v1.0.2
[1.0.3]: https://github.com/SilentBless/wire-repr-rs/releases/tag/v1.0.3
[1.0.4]: https://github.com/SilentBless/wire-repr-rs/releases/tag/v1.0.4
[1.1.0]: https://github.com/SilentBless/wire-repr-rs/releases/tag/v1.1.0
[1.1.1]: https://github.com/SilentBless/wire-repr-rs/releases/tag/v1.1.1
[1.2.0]: https://github.com/SilentBless/wire-repr-rs/releases/tag/v1.2.0
