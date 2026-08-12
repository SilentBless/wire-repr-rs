# Codec contracts and built-in codecs

Codecs connect semantic Rust values to exact wire spans. Generated layouts use codecs for
field-local decode and encoding planning; codecs do not own layout traversal, caller
storage, or cross-field policy.

## Choosing a contract

Use [`FixedCodec`] when every encoded value occupies one nonzero width. Decoding is total
for every exact-width bit pattern. Domain rules such as magic values and reserved ranges
belong to consumer code, not a mandatory fixed-codec validation hook.

Use [`PrefixCodec`] when structural validation must discover a nonzero encoded length from
the available prefix. Validation returns [`PrefixExtent`]; decoding then receives exactly
that accepted span. This preserves legal noncanonical input while allowing [`PrefixCodec::plan`]
to produce a canonical representation.

Both traits return an [`EncodePlan`] from their fallible `plan` operation. A plan reports
its exact output length and performs an infallible write into an exactly-sized slice.
Generated mutation and builders validate every plan, extent, and destination capacity
before committing any caller-buffer write.

## Built-in fixed codecs

The module provides:

- [`U8`] and [`I8`];
- big- and little-endian unsigned 16/24/32/64/128-bit integers;
- big- and little-endian signed 16/32/64/128-bit integers;
- [`Bytes<N>`](Bytes), an opaque borrowed exact-width span.

Unsigned 24-bit codecs use `u32` semantic values and reject values above `0x00ff_ffff`
while planning. `Bytes<N>` requires `N > 0` and checks a builder input's exact width before
mutation.

## Implementor boundary

Custom codec implementations are trusted to follow the laws documented on [`FixedCodec`],
[`PrefixCodec`], and [`EncodePlan`]. Generated layouts defensively check structural claims
that can affect safe slicing or atomic output, but they do not turn a law-violating codec
into a valid one.

A codec should own only one field's wire representation. Framing between fields, bounded byte
ranges, derived endpoints, checksums, and domain relationships remain layout or consumer
responsibilities.
