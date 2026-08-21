# Field codec and preparation contracts

Most schemas use ordinary Rust integers plus `#[wire(be)]` or `#[wire(le)]`. This module
contains the lower-level contracts used by generated code and by custom field
representations.

The runtime is safe Rust, `no_std`, and allocation-free.

## Fixed representations

[`FixedCodec`] describes one field with a nonzero compile-time width:

- `decode` receives exactly [`FixedCodec::WIDTH`] validated bytes;
- `plan` completes every fallible semantic conversion before output mutation;
- the returned [`EncodePlan`] reports exactly that width and writes its complete
  representation into an exact-sized slice.

Built-ins cover one-byte values, big- and little-endian signed and unsigned integers,
24-bit integers, and [`Bytes<N>`](Bytes). Native `[u8; N]` fields are generally clearer
than naming `Bytes<N>` in a derive schema.

The 24-bit codecs use `u32` semantic values and reject values above `0x00ff_ffff` during
planning.

## Self-delimiting representations

[`PrefixCodec`] validates one nonzero leading extent before decoding:

1. `validate_prefix` examines available input and returns an exact [`PrefixExtent`];
2. generated layout validation bounds that extent against the input;
3. `decode` receives exactly the accepted span;
4. `plan` may choose a canonical representation for the semantic value.

A generated view retains the accepted original prefix bytes, so getters have exact
source provenance even when the codec accepts noncanonical input. A prepared write uses
the codec plan rather than copying those source bytes.

## Plans

[`EncodePlan`] is one completed, infallible field write. Its `encoded_len` and
`write_into` methods must describe the same representation.

[`PreparedLayout`] is an aggregate generated plan. `commit_into`:

- checks full output capacity before mutation;
- writes one exact leading representation;
- returns the disjoint unused suffix;
- returns [`OutputTooShortError`] without changing the supplied output when capacity is
  insufficient.

Custom codecs must report widths and extents honestly and make exact-sized writes
complete. These are implementation contracts, not hooks for protocol policy. Magic
values, reserved protocol values, checksums, and cross-field application rules belong to
consumer code.
