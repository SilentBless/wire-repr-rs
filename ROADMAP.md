# Roadmap

`wire-repr` 1.0.1 is the stable baseline. Work below is ordered by risk reduction, not novelty.
Every change preserves `no_std`, no hidden allocation, exact represented ranges, ordinary retained
`AsRef<[u8]>` backing, progressive output ownership, and generated-versus-handwritten evidence.

## Now: repository reliability

These changes do not alter the public wire API.

- Add a downstream package fixture outside the workspace. Its manifest declares only the documented
  `wire-repr` and `thiserror` dependencies, so it cannot inherit workspace dependencies and hide a
  packaging failure. Run it from the path checkout in pull requests and against the packaged crate
  during releases.
- Track the root and fuzz `Cargo.lock` files. The workspace contains executable CI tooling, so
  reproducible tool dependencies matter even though the published libraries do not use lockfiles.
- Make CI run on pull requests and pushes to `master`, not every branch push. Add concurrency
  cancellation and job timeouts. Keep the current four-job shape: quality, measurement, macOS
  artifact evidence, and Windows/PDB artifact evidence.
- Add `cargo-deny` policy for advisories, licenses, duplicate versions, and the single reviewed
  `rzpipe` Git revision. Remove the unused HTTP dependency graph from `rzpipe` by contributing an
  upstream optional feature or using a reviewed pinned revision that makes HTTP optional.
- Enable Dependabot alerts and grouped weekly Cargo and GitHub Actions updates. Keep secret scanning
  and push protection enabled.
- Add `cargo-semver-checks` against the latest published runtime API. Macro syntax and generated APIs
  remain owned by behavior, pass/fail compile fixtures, and expansion tests.
- Add a scheduled Miri job for the unsafe reconstruction boundary and retain the existing
  coverage-guided fuzz workflow. Publish coverage as evidence; do not gate on an arbitrary line
  percentage.
- Add human-facing bug, feature, and performance issue forms, a pull-request template,
  `CONTRIBUTING.md`, and `SECURITY.md`. Enable private vulnerability reporting. Keep Discussions,
  Projects, CODEOWNERS, and a code of conduct out until contributor volume gives them an owner.
- Protect `master` and release tags with a repository ruleset. Require pull requests, linear history,
  resolved conversations, and stable CI check names; forbid force-pushing or deleting release tags.
- Replace long-lived crates.io credentials with Trusted Publishing. Evaluate `release-plz` in dry-run
  mode for one release PR that synchronizes workspace package versions and internal dependency
  versions while leaving the changelog curated by hand.

Exit criteria: a clean checkout can reproduce CI, package both crates, compile the downstream
fixture, prove SemVer compatibility, and publish through short-lived credentials without editing a
tag after publication.

## Next: Rust-native ergonomics

These are additive 1.x candidates. Each gets a focused behavior test and a measurement case when it
can affect generated code.

- Implement `AsRef<[u8]>` and `AsMut<[u8]>` for `Written<O>` and `AsRef<[u8]>` for `ArrayItem<T>`.
  Keep the existing explicit methods for discoverability.
- Implement `IntoIterator` for borrowed physical selections and `FusedIterator` for selection and
  recursive-array iterators where the current state machine already proves fused behavior.
- Migrate `wire-repr-macros` from Syn 2 to Syn 3 if the migration preserves diagnostics. The current
  downstream graph builds both Syn 2 and Syn 3 because `thiserror` already uses Syn 3.
- Add `clippy::undocumented_unsafe_blocks` after moving the existing unit `WireView` safety comment
  directly onto its unsafe implementation. Do not enable the full Clippy pedantic group.

The following custom traits remain deliberate:

- `WireView` retains geometry, a borrowed view family, and the unsafe reconstruction invariant;
  `TryFrom<&[u8]>` cannot express arbitrary owned backing or nested view families.
- `Output` requires contiguous re-borrowing, fallible capacity policy, high-water preservation, and
  backpatching; `std::io::Write` and `Extend<u8>` do not express that contract.
- `ExactWire<T>` carries schema provenance. Plain `AsRef<[u8]>` would let unrelated bytes enter exact
  forwarding paths.
- `ByteSelection` exposes both byte and chunk iterators without materialization. One standard
  `Iterator` cannot preserve both surfaces.
- `WireWrite<V>` writes through a restricted progressive cursor and carries both schema and growth
  errors; `TryFrom` has no output context.

Do not add `Deref`, a generic buffer framework, or wrapper traits that merely rename standard
traits.

## Then: code-generation consolidation

The macro crate currently contains roughly 16,000 source lines. A token-based scan of the schema
renderers found 983 duplicated lines out of 14,483 analyzed lines (6.79%). That is enough to justify
targeted consolidation, not a renderer rewrite.

Before changing renderer structure, record three baselines:

1. normalized expanded Rust for fixed, generic, dynamic, enum, bitfield, selection, and recursive
   compound schemas;
2. cold downstream compile time and dependency graph;
3. the existing 63-case generated/idiomatic/best-safe machine and runtime measurements.

Proceed in this order:

1. Extract exact duplicate utilities: generic argument rendering, scalar type recognition, unique
   public-name allocation, and common generated error bounds.
2. Share the generated retained-view shell (`ViewImpl`, `AsRef`, `ExactWire`, root construction, and
   projected construction) across struct, enum, and nominal-bitfield renderers.
3. Lower nominal bitfields into the existing scalar-plus-projection schema model instead of owning a
   parallel view and builder renderer.
4. Compile recursive object, demand, and array bodies from one finite segment grammar. Keep packed
   continuation payloads specialized by the resulting grammar.
5. Introduce a shared physical layout plan for read framing and progressive writing only after the
   duplicated cursor, alignment, placement, controller, and retained-boundary operations have one
   proven representation.

Every step must preserve source syntax, generated public names, field-site diagnostics, exact error
sources, measurement formulas, and failure offsets. A lower macro line count is not success if
compile time, generated code, or diagnostics regress.

Avoid `darling` or a generic code-generation framework for their own sake. The current Syn
`parse_nested_meta` model owns protocol-specific validation and already produces precise spans.

## Later

- Design transport acquisition and preverification as an adapter around contiguous `WireView`
  framing, not as `Read`/`AsyncRead` ownership in the core.
- Revisit general traversal as an independent composition capability after concrete AST or streaming
  consumers exist.
- Consider unifying detached child builders with progressive child writers only in a major release;
  the current `WireBuilder`/`WireWrite` extension contract is public.

## Issue organization

Use milestones `1.0.x hardening`, `1.1`, and `2.0 research`. Add labels `soundness`, `api`,
`codegen`, `performance`, `diagnostics`, `fuzz`, `ci`, `release`, and `breaking-change`. Roadmap
items become issues only when they have an owner, acceptance criteria, and a verification command;
this file remains the high-level ordering rather than a second backlog.
