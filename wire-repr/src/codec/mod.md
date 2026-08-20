# Codec contracts and built-ins

A codec owns one field's exact wire representation. Layout generation owns physical
order, represented extent, framing, bounded ranges, and caller storage; consumers
own domain policy such as magic values, reserved ranges, checksums, and cross-field
rules.

## Choose a contract

- Implement [`FixedCodec`] when every value has one compile-time, nonzero width.
  Decode every exact-width bit pattern; do not make application semantics a required
  structural-validation step.
- Implement [`PrefixCodec`] when bounded structural validation must discover the
  nonzero encoded width from available input. `validate_prefix` receives the remaining
  bytes and decoding then receives exactly the accepted span.
- Implement [`EncodePlan`] as the result of fallible `plan`. Its `encoded_len` and
  `write_into` must describe the same representation, and writing into its exact-sized
  destination must be infallible.

A prefix view retains the accepted extent. Its raw getter returns those original bytes,
including a legal noncanonical encoding; it does not reconstruct them from the decoded
value. A prefix plan may choose a canonical encoding for a value.

## Builder boundary

Generated setters and builders use plans to preserve atomic caller-buffer updates.
Before a builder writes, it completes all fallible planning, plan-length checks,
derivations, endpoint calculations, arithmetic, and capacity checks. A returned build
error therefore leaves the complete supplied output slice unchanged.

Generated code defensively verifies claims that affect safe slicing and atomicity, but
a custom codec that violates its trait laws is still broken. In particular:

- `FixedCodec::WIDTH` is nonzero, and every successful fixed plan is exactly that width.
- `PrefixCodec::validate_prefix` returns a nonzero extent within its supplied input.
- Every successful prefix plan is nonempty.
- A successful plan encodes the semantic value it planned.

Keep framing between fields, range-source algebra, derived endpoints, and protocol
validation outside a codec. A prefix codec discovers its own field width; it is not a
source for a later dynamic byte range.

## Built-ins

This module provides [`U8`] and [`I8`], big- and little-endian unsigned
16/24/32/64/128-bit codecs, big- and little-endian signed 16/32/64/128-bit codecs,
and [`Bytes<N>`](Bytes) for an opaque exact-width borrowed span.

Unsigned 24-bit codecs use `u32` semantic values and reject values greater than
`0x00ff_ffff` while planning. `Bytes<N>` requires `N > 0`; building checks that an
input has exactly `N` bytes before mutation.
