# Roadmap

Open work only, ordered by what it buys a user. Finished work lives in `CHANGELOG.md`. Every change
preserves `no_std`, no hidden allocation, exact represented ranges, ordinary retained
`AsRef<[u8]>` backing, progressive output ownership, and derived-versus-handwritten evidence.

## Now: close the measured gaps

The measurement corpus reports 0 errors and 33 attention findings. The largest gaps between
derived code and the independent handwritten implementation are below. Numbers are median
nanoseconds from one `aarch64-apple-darwin` run; rerun before starting work.

| Case | Derived | Handwritten |
| --- | ---: | ---: |
| `selection/fragmented` bytes | 4.85 | 1.51 |
| `selection/fragmented` nested bytes and chunks | 5.4–6.3 | 1.9–2.1 |
| `computed/selection` fixed and dynamic | 2.8–3.1 | 1.1–1.3 |
| `recursive/json` iterate 200 | 8326 | 2789 |
| `recursive/json` demand view | 36.9 | 14.2 |
| `recursive/json` geometry, recursive shape | 46489 | 2431 |
| `output/growth` growable and owned | 20.9–21.4 | 16.5–16.8 |
| `scalar/uleb128` decode | 213 | 179 |

Take them in this order: selections, because checksums use them on every packet; then recursive
iteration; then the rest. `SelectionChunks` rescans every selected range to find each next chunk,
which is the first thing to measure. Each fix needs its attention rule to pass and no regression
elsewhere in the corpus.

## Next: cost of adoption

- Print fields from generated views. `dbg!(packet)` should show field names and values. This needs
  `Debug` on the opaque `impl SchemaView` return type and a bound rule for generic children; settle
  that rule before generating anything.
- Resolve the runtime crate without `proc-macro-crate`. It brings `toml_edit`, `winnow`,
  `indexmap`, and `hashbrown` into every consumer's build only to support a renamed dependency.
  Emit `::wire_repr` paths and accept `#[wire(crate = path)]` for the renamed case. This changes
  behavior for consumers who rename the dependency today, so it needs a release note.
- Migrate `wire-repr-macros` from Syn 2 to Syn 3 if diagnostics survive unchanged. Consumers
  currently build both because `thiserror` already uses Syn 3.
- Measure cold compile time of the downstream fixture and record it next to the runtime numbers,
  so the two items above have a baseline.

## Then: repository reliability

- Bring the six tests the Miri job skips (`recursive::recursive_geometry*`, `structural_fuzz`) under
  interpretation with reduced input sizes; each currently runs for tens of minutes there.
- Fix `wire-repr-measure run --no-runtime`: it fails on any workload whose formulas read runtime
  medians, so artifact-only comparison is unavailable.
- Add a `cargo-deny` policy. A trial run found three things to decide first:
  - `evalexpr`, used only by the unpublished measurement tool, is `AGPL-3.0-only`. Replace it or
    record why it is acceptable for tooling that is never distributed.
  - `rzpipe` pulls an HTTP stack the tool never uses; it carries RUSTSEC-2026-0258 (`h2`) and an
    unmaintained `rustls-pemfile`. Make HTTP optional upstream or pin a revision that does.
  - The published crates' own dependencies are MIT/Apache-2.0, 0BSD (`vu128`), and Unicode-3.0
    (`unicode-ident`).
- Add `clippy::undocumented_unsafe_blocks` after moving the unit `WireView` safety comment onto its
  unsafe implementation. Do not enable the full Clippy pedantic group.
- Protect `master` and release tags with a ruleset: required CI checks, no force-push, no tag
  deletion. Decide whether direct pushes stay allowed for the sole maintainer.
- Publish through crates.io Trusted Publishing instead of a long-lived token. Evaluate
  `release-plz` in dry-run mode for version and changelog synchronization.

## Then: code-generation consolidation

The macro crate is about 16,000 lines. A token-based scan found 6.79% duplicated lines, which
justifies targeted consolidation, not a rewrite. The pure-refactor gate is described in
`CONTRIBUTING.md`: the expanded test suite must stay byte-identical.

1. Extract exact duplicate utilities: generic argument rendering, scalar type recognition, unique
   public-name allocation, and common generated error bounds.
2. Share the generated retained-view shell (`ViewImpl`, `AsRef`, `ExactWire`, root construction)
   across struct, enum, and nominal-bitfield renderers. The three shells differ today in hygiene and
   inlining attributes, so this step changes expansion and needs the measurement corpus as its gate.
3. Lower nominal bitfields into the scalar-plus-projection schema model instead of a parallel
   renderer.
4. Compile recursive object, demand, and array bodies from one finite segment grammar.
5. Introduce a shared physical layout plan for read framing and progressive writing only after the
   duplicated cursor, alignment, placement, and controller operations have one proven
   representation.

Every step preserves source syntax, generated public names, field-site diagnostics, error sources,
and failure offsets. Fewer macro lines is not success if compile time, generated code, or
diagnostics regress.

Avoid `darling` or a generic code-generation framework. The Syn `parse_nested_meta` model owns
protocol-specific validation and already produces precise spans.

## Later

- Transport acquisition and preverification as an adapter around contiguous `WireView` framing, not
  `Read`/`AsyncRead` ownership in the core.
- General traversal as an independent capability once concrete AST or streaming consumers exist.
- Unifying detached child builders with progressive child writers, in a major release only; the
  `WireBuilder`/`WireWrite` extension contract is public.

## Deliberately custom traits

These do not become standard traits:

- `WireView` retains geometry, a borrowed view family, and the unsafe reconstruction invariant.
  `TryFrom<&[u8]>` cannot express owned backing or nested view families.
- `Output` needs contiguous re-borrowing, a fallible capacity policy, and backpatching.
  `std::io::Write` and `Extend<u8>` do not express that.
- `ExactWire<T>` carries schema provenance. Plain `AsRef<[u8]>` would let unrelated bytes enter
  exact forwarding paths.
- `ByteSelection` exposes byte and chunk iterators without materialization. `IntoIterator` yields
  chunks; the trait keeps both.
- `WireWrite<V>` writes through a restricted cursor and carries schema and growth errors.
  `TryFrom` has no output context.

Do not add `Deref`, a generic buffer framework, or wrapper traits that only rename standard ones.
