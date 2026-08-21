# Codec contracts and built-ins

A codec owns one field's exact wire representation. Layout generation owns physical
order, represented extent, framing, bounded ranges, and caller storage; consumers own
domain policy such as magic values, reserved ranges, checksums, and cross-field rules.

## Choose a contract

- Implement [`FixedCodec`] when every value has one compile-time, nonzero width.
  Decode every exact-width bit pattern; do not make application semantics a required
  structural-validation step. Declare such a codec directly with `name: path::Codec;`.
- Implement [`PrefixCodec`] when bounded structural validation must discover the
  nonzero encoded width from available input. `validate_prefix` receives the remaining
  bytes and decoding then receives exactly the accepted span. Declare it with
  `name: variable(path);`.
- Implement [`EncodePlan`] as the result of fallible `plan`. Its `encoded_len` and
  `write_into` must describe the same representation, and writing into its exact-sized
  destination must be infallible.

A self-delimiting view retains its accepted extent. Its raw getter returns those
original bytes, including a legal noncanonical encoding; it does not reconstruct them
from the decoded value. A self-delimiting plan may choose a canonical encoding for a
value.

## Builder boundary

Generated setters and builders use plans to preserve atomic caller-buffer updates.
Before a builder writes, it completes all fallible planning, plan-length checks,
derivations, endpoint calculations, arithmetic, and capacity checks. A returned build
error therefore leaves the complete supplied output slice unchanged.

Generated code defensively verifies claims that affect safe slicing and atomicity, but
a custom codec that violates its trait laws is still broken. In particular:

- `FixedCodec::WIDTH` is nonzero, and every successful fixed plan is exactly that width.
- `PrefixCodec::validate_prefix` returns a nonzero extent within its supplied input.
- Every successful self-delimiting plan is nonempty.
- A successful plan encodes the semantic value it planned.

Keep framing between fields, range-source algebra, derived endpoints, and protocol
validation outside a codec. A self-delimiting codec discovers its own field width; it
is not a source for a later dynamic byte range.

## Range-source adapters

[`RangeSource`] performs checked bidirectional structural conversion between a decoded
fixed source representation and byte geometry. `to_bytes` supplies either a relative
length or an exclusive representation-relative endpoint while parsing; `from_bytes`
canonicalizes the encoded source from builder-requested geometry during preparation.
Supported values and geometries must round-trip coherently, with checked arithmetic
and an explicit conversion error. The trait does not decide protocol policy.

For a `range_source: Adapter` macro field, parsing calls `to_bytes` at each consuming
range before normal checked range/bounds validation. Preparation derives geometry,
requires shared consumers to agree by geometry, converts once per source with
`from_bytes`, then plans the physical codec. Commit only checks capacity and writes the
prepared plan, preserving atomic caller-output updates. No API retains contradictory or
noncanonical source values.

Macro adapters are restricted to direct built-in integer fixed fields that physically
precede a consumed range. They cannot coexist with mappings, projections, derivation,
or finalization. This hard ownership boundary avoids custom `FixedCodec` values or
plans whose borrows would require self-referential prepared storage. The source getter
is still its raw wire integer; adapters do not add a byte-geometry getter or protocol
semantics.

## Built-ins

This module provides [`U8`] and [`I8`], big- and little-endian unsigned
16/24/32/64/128-bit codecs, big- and little-endian signed 16/32/64/128-bit codecs,
and [`Bytes<N>`](Bytes) for an opaque exact-width borrowed span.

Unsigned 24-bit codecs use `u32` semantic values and reject values greater than
`0x00ff_ffff` while planning. `Bytes<N>` requires `N > 0`; building checks that an
input has exactly `N` bytes before mutation.
