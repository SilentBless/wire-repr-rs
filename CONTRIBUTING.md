# Contributing

The pinned toolchain is Rust 1.91.0 (`rust-toolchain.toml`). CI runs the commands below; a change
is ready when they pass locally.

## Checks

```text
cargo fmt --all -- --check
cargo test --locked --workspace --all-targets
cargo test --locked -p wire-repr --doc
cargo clippy --locked --workspace --all-targets -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --locked --workspace --no-deps
python3 ci/check-fail-fast.py
CARGO_TARGET_DIR=target cargo run --manifest-path ci/downstream/Cargo.toml
```

- `ci/check-fail-fast.py` compiles every file in `wire-repr/tests/fail_fast/` and requires the
  diagnostic named in its header. Add a fixture there for each new rejected declaration.
- `ci/downstream` is a standalone consumer outside the workspace. It declares only the documented
  dependencies and forbids unsafe code, so it catches packaging and lint regressions.

## Measurement

```text
cargo run --locked -p wire-repr-measure --release -- run
cargo run --locked -p wire-repr-measure --release -- run --workload fixed/scalars
```

The harness needs Rizin 0.9.1 on `PATH`, or `WIRE_REPR_RIZIN` pointing at the executable. Each
workload under `wire-repr/measure/` holds a derived implementation, an independent handwritten one,
and a best-safe one, plus the rules that gate them. A new layout class needs a workload. Instruction
counts explain a result; the runtime rules decide it.

## Fuzzing

```text
cargo +nightly-2026-08-01 fuzz run recursive -- -max_total_time=20 -max_len=512
```

Targets live under `fuzz/`: `view_compound`, `builder_roundtrip`, `recursive`, and
`sequence_cursor`. Pull requests run each for 20 seconds; a nightly schedule runs 120 seconds.
Commit a minimized failure as a regression test, not as corpus.

## Changing generated code

The derive output is the product. When a change is meant to be a pure refactor of
`wire-repr-macros`, compare the expansion before and after:

```text
cargo +nightly-2026-08-01 rustc -p wire-repr --test suite -- -Zunpretty=expanded > expanded.rs
```

A refactor leaves that file byte-identical. A change that alters it needs a behavior test and,
when it can affect the hot path, a measurement case.

## Commits and releases

Use [Conventional Commits](https://www.conventionalcommits.org/). Record user-visible changes
under `Unreleased` in `CHANGELOG.md`. Both crates share one version in the workspace manifest.
