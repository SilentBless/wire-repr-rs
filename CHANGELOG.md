# Changelog

All notable public changes to `wire-repr` are recorded here.

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

[1.0.0]: https://github.com/SilentBless/wire-repr-rs/releases/tag/v1.0.0
