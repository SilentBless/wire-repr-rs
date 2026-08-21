# Architecture

This document defines the `wire-repr` 1.0 representation model. It describes current
behavior, not compatibility history or a roadmap.

## 1. Ownership

`#[derive(Wire)]` connects three deliberately different owners:

```text
Foo<'value>      semantic value and write intent
FooView<'wire>   validated, bytes-backed read representation
FooPlan<'value>  prepared atomic encoding
```

The user-declared struct or enum remains an ordinary Rust value. Reading does not
construct it. A generated view borrows the original input, retains exact represented
bytes and validated dynamic geometry, and exposes getters. A plan owns or borrows every
piece of completed write state needed for an infallible commit.

This separation avoids pretending that Rust memory layout is a wire ABI. It also keeps
reading allocation-free and preserves exact source framing without copying fields into a
second semantic object.

The runtime is `no_std`, allocation-free, safe Rust, and has no runtime schema or dynamic
dispatch layer.

## 2. Struct representation

Declaration order is physical order. Supported field forms are intentionally narrow:

| Rust field and attribute | Physical representation |
| --- | --- |
| `u8`, `i8` | One byte. |
| Multibyte integer with `#[wire(be)]` or `#[wire(le)]` | Fixed-width integer in the selected byte order. |
| `[u8; N]` | Exactly `N` bytes; the view getter borrows `&[u8; N]`. |
| Nested `Wire` type | The nested type's complete representation and generated nested view. |
| `#[wire(codec = Path)]` | A custom fixed-width `FixedCodec`. |
| `#[wire(prefix = Path)]` | One self-delimiting `PrefixCodec` representation. |
| `#[wire(bytes = source)]` | A borrowed byte slice bounded by an earlier unsigned source field. |
| `#[wire(rest)]` | A borrowed terminal slice. |
| `pad_before`, `align_before` | Opaque physical gap before the field. |
| `#[wire(at = position)]` | Forward placement at a literal or earlier unsigned absolute position. |

A bounded byte source is decoded once while geometry is validated. Getters use the
retained range. During preparation the byte slice is authoritative: its length is
converted into the source value and planned canonically. A stale semantic source value
does not create contradictory output.

A prefix source may control bounded bytes. The prefix codec validates its exact source
span before its decoded unsigned value is used for geometry. The generated view retains
the original prefix bytes, including accepted noncanonical encodings; writing uses the
codec's prepared representation.

Padding, alignment, and forward gaps are accepted as opaque bytes on read and
canonicalized to zero on write. Backward placement, arithmetic overflow, and input
shortage are structural failures.

## 3. Views and framing

`Type::view(input)` creates a request with two terminals:

- `with_remainder()` validates one leading representation and returns its generated view
  plus the disjoint suffix;
- `without_trailing()` performs the same one-pass validation and rejects a nonempty
  suffix.

`FooView::as_bytes()` returns exactly the represented bytes. Scalar getters decode from
retained exact spans. Nested and enum-body getters return retained child views. Dynamic
byte getters use endpoints established during the framing pass rather than re-reading
controller fields.

Views are borrowed and immutable. Mutation is expressed by constructing a semantic write
value and preparing it, not by a parallel setter or mutable-view hierarchy.

Structural validation establishes bounds, field extents, prefix claims, tag selection,
and physical geometry. It does not validate protocol meaning such as magic values,
reserved protocol values, checksums, cryptographic hashes, or application state.

## 4. Tagged enums and opcodes

A static enum declares a typed tag representation and an explicit unknown policy. Integer
tag codecs use explicit discriminants. Fixed byte tags use exact `b"..."` selectors whose
width matches `[u8; N]`; they are never reinterpreted as integers or text. Known variants
are unit-like or carry one nested wire value, and the tag precedes the selected body. Rust
`repr` satisfies Rust's discriminant rules but does not select integer wire width or byte
order.

Generated enum views retain exact bytes and private dispatch state. Unit variants expose
predicates; body variants expose `Option<BodyView<'wire>>`. `unknown = reject` returns the
lossless raw tag in the decode error. `unknown = preserve` requires one explicit
`#[wire(unknown)]` raw-tag variant and can round-trip it byte-for-byte. It preserves the tag
only: an unknown body boundary is never inferred.

Negotiated IDs use one concrete consumer-owned opcode table named by the enum. The table
provides raw-to-semantic and semantic-to-raw inherent methods. It is supplied explicitly
through `.opcodes(&table)` at view and preparation boundaries. One outer struct can pass
the same table through several fields marked `#[wire(opcodes)]`.

Opcode mapping is not an ambient context framework. The table is used during validation
or preparation and is not retained in a generated view or plan. Mapping failure remains
distinct from an unknown raw ID.

## 5. Nominal bitfields

A bitfield is a separate semantic struct with:

```text
#[wire(bitfield = unsigned_storage, byte_order, reserved = zero)]
```

Each named field owns one non-overlapping `bit` or inclusive `bits` projection. The
macro validates storage width, projection bounds, overlap, and semantic field width.
Bit positions use least-significant-bit numbering after decoding the storage scalar;
byte order and bit numbering are separate concepts.

The generated `FlagsView` owns the one physical scalar span. Logical projections are
getters over that span, not overlapping physical fields on a parent packet. Unprojected
bits are accepted on read and canonicalized to zero on write. The policy is explicit so
future policies cannot silently change existing schemas.

## 6. Sequences

There are two sequence contracts because their evidence differs:

- A statically fixed, plain sequential representation exposes `Type::views(bytes)`. One
  initial check rejects a trailing partial item, then `FixedViewIterator` yields views
  infallibly and implements `ExactSizeIterator`.
- A potentially variable representation exposes `Type::cursor(bytes)`. `ViewCursor::next`
  returns `Result<Option<View>, Error>`, validates one item at a time, and never advances
  on failure. A successful zero-width item is rejected to prevent a non-progress loop.

The variable cursor intentionally does not implement `Iterator`: item boundary failure is
part of control flow and must not be hidden inside `Option` or an allocated index.

This sequence surface reads consecutive complete representations. It does not pretend
that an arbitrary runtime-length slice of variable-width semantic values can become an
atomic outer plan without storing their prepared child plans. That write-side problem
requires a concrete owner and is not generalized speculatively.

## 7. Codec contracts

`FixedCodec` owns a nonzero compile-time width, value conversion, and a fallible prepared
plan. `PrefixCodec` validates one nonzero accepted extent before decoding exactly that
span. `EncodePlan` reports one exact length and writes that complete representation into
an exact-sized output.

Generated views call codecs only on validated spans. Generated aggregate preparation
creates all codec plans before output mutation. A custom codec violating width, extent,
or write contracts is a broken implementation, not a protocol-validation mechanism.

## 8. Prepared atomic writes

Writing has two stages:

1. `value.prepare()` consumes semantic write intent and returns `FooPlan` or a typed
   preparation error.
2. `plan.commit_into(output)` checks complete capacity, writes one represented prefix,
   and returns `Written` plus the disjoint untouched suffix.

Preparation completes every fallible operation: codec planning, canonical controller
derivation, opcode mapping, conversions, range and placement checks, child preparation,
and total-length arithmetic. The plan retains the resulting state. Commit does not
reparse, remap, or repeat fallible planning.

Short output is checked before the first write and leaves the entire supplied slice
unchanged. Successful commit writes only the represented prefix. `build_into` is the
prepare-and-commit convenience path with the same guarantee.

## 9. Responsibility boundary

`wire-repr` owns physical representation: widths, byte order, field order, tags,
self-delimiting extents, borrowed ranges, placement, framing, and atomic output.
Consumers own protocol and application semantics.

The PNG, SQLite, and WebAssembly fixtures demonstrate this split. Their derived layouts
own bytes and geometry; their handwritten code owns chunk-name rules, CRC checks, SQLite
header policy, and WebAssembly meaning.
