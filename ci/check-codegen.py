#!/usr/bin/env python3
"""Discover codegen fixtures and score generated/handwritten release pairs."""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

METRICS = ("instructions", "branches", "calls", "panic_paths")
DEFAULT_WEIGHTS = {
    "instructions": 1.0,
    "branches": 4.0,
    "calls": 8.0,
    "panic_paths": 16.0,
}
PAIR = re.compile(
    r"^//! pair:\s*(?P<name>[A-Za-z][A-Za-z0-9_-]*)\s*=\s*"
    r"(?P<generated>[A-Za-z_][A-Za-z0-9_]*)\s*/\s*"
    r"(?P<handwritten>[A-Za-z_][A-Za-z0-9_]*)\s*$"
)
TOLERANCE = re.compile(r"^//! tolerance:\s*(?P<value>[^\s]+)\s*$")
WEIGHTS = re.compile(r"^//! weights:\s*(?P<value>.+?)\s*$")
MODE = re.compile(r"^//! mode:\s*(?P<value>default|bytes)\s*$")
METADATA = re.compile(r"^//!\s*(?P<key>[A-Za-z][A-Za-z0-9_-]*):")
PANIC_MARKERS = (
    "panic",
    "bounds_check",
    "slice_index",
    "copy_from_slice",
    "assert_failed",
)
FORBIDDEN_MARKERS = ("__rust_alloc", "__rust_realloc", "__rust_dealloc", "vtable")


@dataclass(frozen=True)
class Pair:
    fixture: str
    name: str
    generated: str
    handwritten: str

    @property
    def identity(self) -> str:
        return f"{self.fixture}/{self.name}"


@dataclass(frozen=True)
class Fixture:
    path: Path
    pairs: tuple[Pair, ...]
    tolerance: float | None
    weights: dict[str, float]
    mode: str


def percentage(value: str, context: str) -> float:
    if not value.endswith("%"):
        raise RuntimeError(f"{context}: tolerance must use a `%` suffix")
    try:
        result = float(value[:-1]) / 100.0
    except ValueError as error:
        raise RuntimeError(f"{context}: invalid tolerance {value!r}") from error
    if not math.isfinite(result) or result < 0.0:
        raise RuntimeError(f"{context}: tolerance must be finite and nonnegative")
    return result


def weight_map(value: str, context: str) -> dict[str, float]:
    result: dict[str, float] = {}
    for assignment in value.split(","):
        key, separator, raw = assignment.strip().partition("=")
        if not separator or key not in METRICS:
            raise RuntimeError(f"{context}: invalid weight assignment {assignment!r}")
        if key in result:
            raise RuntimeError(f"{context}: duplicate weight {key!r}")
        try:
            parsed = float(raw)
        except ValueError as error:
            raise RuntimeError(f"{context}: invalid weight {raw!r}") from error
        if not math.isfinite(parsed) or parsed < 0:
            raise RuntimeError(f"{context}: weights must be finite and nonnegative")
        result[key] = parsed
    if not result or not any(result.values()):
        raise RuntimeError(f"{context}: at least one weight must be positive")
    return result


def parse_fixture(root: Path, path: Path) -> Fixture:
    name = path.stem
    pairs: list[Pair] = []
    tolerance: float | None = None
    weights: dict[str, float] = {}
    mode = "default"
    mode_declared = False
    for line_number, line in enumerate(path.read_text().splitlines(), 1):
        context = f"{path.relative_to(root)}:{line_number}"
        if match := PAIR.match(line):
            pairs.append(
                Pair(
                    name,
                    match.group("name"),
                    match.group("generated"),
                    match.group("handwritten"),
                )
            )
        elif match := TOLERANCE.match(line):
            if tolerance is not None:
                raise RuntimeError(f"{context}: duplicate tolerance")
            tolerance = percentage(match.group("value"), context)
        elif match := WEIGHTS.match(line):
            if weights:
                raise RuntimeError(f"{context}: duplicate weights")
            weights = weight_map(match.group("value"), context)
        elif match := MODE.match(line):
            if mode_declared:
                raise RuntimeError(f"{context}: duplicate mode")
            mode = match.group("value")
            mode_declared = True
        elif match := METADATA.match(line):
            raise RuntimeError(f"{context}: unknown metadata key {match.group('key')!r}")
    if not pairs:
        raise RuntimeError(f"{path.relative_to(root)}: no `//! pair:` metadata")
    identities = [pair.name for pair in pairs]
    if len(identities) != len(set(identities)):
        raise RuntimeError(f"{path.relative_to(root)}: duplicate pair name")
    return Fixture(path, tuple(pairs), tolerance, weights, mode)


def discover(root: Path, mode: str) -> tuple[Fixture, ...]:
    directory = root / "wire-repr" / "tests" / "fixtures"
    paths = sorted(directory.glob("*.rs"))
    if not paths:
        raise RuntimeError(f"no codegen fixtures found below {directory}")
    all_fixtures = tuple(parse_fixture(root, path) for path in paths)
    pairs = [pair for fixture in all_fixtures for pair in fixture.pairs]
    identities = [pair.identity for pair in pairs]
    symbols = [symbol for pair in pairs for symbol in (pair.generated, pair.handwritten)]
    if len(identities) != len(set(identities)):
        raise RuntimeError("duplicate fixture/pair identity")
    if len(symbols) != len(set(symbols)):
        raise RuntimeError("generated and handwritten symbols must be globally unique")
    fixtures = tuple(fixture for fixture in all_fixtures if fixture.mode == mode)
    if not fixtures:
        raise RuntimeError(f"no codegen fixtures found for mode {mode!r}")
    return fixtures


def harness(root: Path, fixtures: tuple[Fixture, ...], mode: str) -> Path:
    crate = root / "target" / "codegen-fixtures" / f"harness-{mode}"
    wire_features = ', features = ["bytes"]' if mode == "bytes" else ""
    tests = crate / "tests"
    tests.mkdir(parents=True, exist_ok=True)
    (crate / "src").mkdir(parents=True, exist_ok=True)
    (crate / "src" / "lib.rs").write_text("")
    (crate / "Cargo.toml").write_text(
        "\n".join(
            [
                "[package]",
                'name = "wire-repr-codegen-fixtures"',
                'version = "0.0.0"',
                'edition = "2024"',
                "",
                "[dependencies]",
                f'wire-repr = {{ path = {json.dumps(str(root / "wire-repr"))}{wire_features} }}',
                *(('bytes = { version = "1", default-features = false }',) if mode == "bytes" else ()),
                "",
                "[workspace]",
                "",
                "[[test]]",
                'name = "codegen_harness"',
                'path = "tests/codegen_harness.rs"',
                "",
            ]
        )
    )
    modules = [
        f'#[path = {json.dumps(str(fixture.path))}] mod fixture_{index};'
        for index, fixture in enumerate(fixtures)
    ]
    (tests / "codegen_harness.rs").write_text("\n".join(modules) + "\n")
    return crate


def compile_harness(root: Path, crate: Path, target: str, toolchain: str) -> tuple[Path, Path]:
    target_dir = root / "target" / "codegen-fixtures" / "build"
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(target_dir)
    subprocess.run(
        ["cargo", f"+{toolchain}", "test", "--manifest-path", str(crate / "Cargo.toml"), "--test", "codegen_harness"],
        cwd=root,
        env=environment,
        check=True,
    )
    if sys.platform == "darwin" and target.endswith("-unknown-linux-gnu"):
        variable = f"CARGO_TARGET_{target.upper().replace('-', '_')}_LINKER"
        environment.setdefault(variable, "/usr/bin/true")
    subprocess.run(
        [
            "cargo",
            f"+{toolchain}",
            "rustc",
            "--manifest-path",
            str(crate / "Cargo.toml"),
            "--test",
            "codegen_harness",
            "--release",
            "--target",
            target,
            "--",
            "--emit=asm,llvm-ir",
        ],
        cwd=root,
        env=environment,
        check=True,
    )
    deps = target_dir / target / "release" / "deps"
    ir_files = sorted(deps.glob("codegen_harness-*.ll"), key=lambda path: path.stat().st_mtime)
    if not ir_files:
        raise RuntimeError(f"expected optimized LLVM IR below {deps}")
    ir_path = ir_files[-1]
    assembly_path = ir_path.with_suffix(".s")
    if not assembly_path.is_file():
        raise RuntimeError(f"expected matching assembly at {assembly_path}")
    return ir_path, assembly_path


def unquote(symbol: str) -> str:
    return symbol.strip('"')


def aliases(ir: str) -> dict[str, str]:
    result: dict[str, str] = {}
    pattern = re.compile(r'^@(?P<alias>"?[\w.$-]+"?)\s*=\s*alias\b.*?@(?P<target>"?[\w.$-]+"?)\s*$', re.MULTILINE)
    for match in pattern.finditer(ir):
        result[unquote(match.group("alias"))] = unquote(match.group("target"))
    return result


def definitions(ir: str) -> dict[str, str]:
    result: dict[str, str] = {}
    header = re.compile(r'^define\b.*?@(?P<symbol>"?[\w.$-]+"?)\(', re.MULTILINE)
    for match in header.finditer(ir):
        start = ir.find("{", match.end())
        if start < 0:
            continue
        depth = 0
        for end in range(start, len(ir)):
            if ir[end] == "{":
                depth += 1
            elif ir[end] == "}":
                depth -= 1
                if depth == 0:
                    result[unquote(match.group("symbol"))] = ir[start + 1 : end]
                    break
    return result


def resolve(symbol: str, alias_map: dict[str, str]) -> str:
    seen: set[str] = set()
    while symbol in alias_map and symbol not in seen:
        seen.add(symbol)
        symbol = alias_map[symbol]
    return symbol


def locate(marker: str, bodies: dict[str, str], alias_map: dict[str, str]) -> tuple[str, str] | None:
    candidates = sorted({symbol for symbol in (*bodies, *alias_map) if marker in symbol})
    available = [(symbol, resolve(symbol, alias_map)) for symbol in candidates if resolve(symbol, alias_map) in bodies]
    if not candidates:
        return None
    if len(available) != 1:
        names = ", ".join(f"{symbol}->{target}" for symbol, target in available)
        raise RuntimeError(f"expected one symbol containing {marker!r}; found {names}")
    symbol, target = available[0]
    return symbol, bodies[target]


def call_references(assembly: str, symbol: str) -> int:
    pattern = rf"\b(?:callq?|bl)\s+_?{re.escape(symbol)}(?:@PLT)?\b"
    return len(re.findall(pattern, assembly))


def metrics(body: str) -> dict[str, int]:
    lines = [line.strip() for line in body.splitlines() if line.strip() and not line.lstrip().startswith(";")]
    calls = [line for line in lines if re.search(r"\b(?:call|invoke)\b", line) and "llvm." not in line]
    runtime = [line for line in lines if "llvm." not in line]
    return {
        "instructions": sum(1 for line in runtime if not line.endswith(":")),
        "branches": sum(1 for line in lines if line.startswith("br ") or line.startswith("switch ")),
        "calls": len(calls),
        "panic_paths": sum(1 for line in calls if any(marker in line for marker in PANIC_MARKERS)),
    }


def cost(values: dict[str, int], weights: dict[str, float]) -> float:
    return sum(values[name] * weights[name] for name in METRICS)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--target", default="x86_64-unknown-linux-gnu")
    parser.add_argument("--toolchain", default="1.91.0")
    parser.add_argument("--tolerance", default="10%")
    parser.add_argument("--mode", choices=("default", "bytes"), default="default")
    args = parser.parse_args()
    root = args.root.resolve()
    default_tolerance = percentage(args.tolerance, "--tolerance")
    fixtures = discover(root, args.mode)
    crate = harness(root, fixtures, args.mode)
    ir_path, assembly_path = compile_harness(root, crate, args.target, args.toolchain)
    ir = ir_path.read_text()
    assembly = assembly_path.read_text()
    bodies = definitions(ir)
    alias_map = aliases(ir)
    failures: list[str] = []

    for fixture in fixtures:
        weights = DEFAULT_WEIGHTS | fixture.weights
        tolerance = fixture.tolerance if fixture.tolerance is not None else default_tolerance
        for pair in fixture.pairs:
            try:
                generated = locate(pair.generated, bodies, alias_map)
                handwritten = locate(pair.handwritten, bodies, alias_map)
            except RuntimeError as error:
                failures.append(f"{pair.identity}: {error}")
                continue
            if generated is None or handwritten is None:
                survivor = generated or handwritten
                if survivor is None or call_references(assembly, survivor[0]) < 2:
                    failures.append(f"{pair.identity}: pair symbols did not both survive or merge verifiably")
                    continue
                generated_values = handwritten_values = metrics(survivor[1])
                generated_body = survivor[1]
            else:
                generated_values = metrics(generated[1])
                handwritten_values = metrics(handwritten[1])
                generated_body = generated[1]

            forbidden = [marker for marker in FORBIDDEN_MARKERS if marker in generated_body]
            generated_cost = cost(generated_values, weights)
            handwritten_cost = cost(handwritten_values, weights)
            if handwritten_cost == 0:
                overhead = 0.0 if generated_cost == 0 else math.inf
            else:
                overhead = (generated_cost - handwritten_cost) / handwritten_cost
            print(
                f"{pair.identity}: generated={generated_values} handwritten={handwritten_values} "
                f"weights={weights} costs={generated_cost:g}/{handwritten_cost:g} "
                f"overhead={overhead:.2%} tolerance={tolerance:.2%}"
            )
            if forbidden:
                failures.append(f"{pair.identity}: generated body contains forbidden markers {forbidden}")
            if overhead > tolerance:
                failures.append(f"{pair.identity}: weighted overhead {overhead:.2%} exceeds {tolerance:.2%}")

    if failures:
        print("codegen fixture gate failed:", file=sys.stderr)
        print("\n".join(f"  - {failure}" for failure in failures), file=sys.stderr)
        return 1
    print(f"codegen fixture gate passed ({sum(len(fixture.pairs) for fixture in fixtures)} pairs)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, subprocess.CalledProcessError, RuntimeError) as error:
        print(f"codegen fixture gate failed: {error}", file=sys.stderr)
        raise SystemExit(1)