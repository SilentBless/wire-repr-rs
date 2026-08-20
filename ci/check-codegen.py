#!/usr/bin/env python3
"""Compare release IR for generated and handwritten codegen probes.

This deliberately compares pair-local structure rather than whole assembly files:
register allocation, labels, instruction scheduling, and unrelated test-harness code are
not a contract. The integration test itself provides the semantic oracle.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

PAIRS = (
    ("fixed_getter", "generated_fixed_getter", "handwritten_fixed_getter", True, 2),
    ("projection", "generated_projection", "handwritten_projection", True, 8),
    ("mutation", "generated_mutation", "handwritten_mutation", True, 4),
    ("builder", "generated_builder", "handwritten_builder", False, 4),
    ("prepared_builder", "generated_prepared_builder", "handwritten_prepared_builder", False, 4),
    ("dynamic_prepared_builder", "generated_dynamic_prepared_builder", "handwritten_dynamic_prepared_builder", False, 4),
    ("absolute_prepared_builder", "generated_absolute_prepared_builder", "handwritten_absolute_prepared_builder", False, 4),
    ("scalar_getter", "generated_scalar_getter", "handwritten_scalar_getter", True, 8),
    ("scalar_mutation", "generated_scalar_mutation", "handwritten_scalar_mutation", True, 4),
    ("scalar_builder", "generated_scalar_builder", "handwritten_scalar_builder", True, 4),
    ("mapped_getter", "generated_mapped_getter", "handwritten_mapped_getter", True, 8),
    ("mapped_mutation", "generated_mapped_mutation", "handwritten_mapped_mutation", True, 4),
    ("mapped_builder", "generated_mapped_builder", "handwritten_mapped_builder", True, 4),
    ("relative_range_getter", "generated_relative_range_getter", "handwritten_relative_range_getter", False, 8),
    ("relative_range_mutation", "generated_relative_range_mutation", "handwritten_relative_range_mutation", False, 4),
    ("absolute_range_getter", "generated_absolute_range_getter", "handwritten_absolute_range_getter", False, 8),
    ("absolute_range_mutation", "generated_absolute_range_mutation", "handwritten_absolute_range_mutation", False, 4),
    ("absolute_range_builder", "generated_absolute_range_builder", "handwritten_absolute_range_builder", False, 6),
    ("derived_range_builder", "generated_derived_range_builder", "handwritten_derived_range_builder", False, 2),
    ("finalizer_builder", "generated_finalizer_builder", "handwritten_finalizer_builder", True, 4),
    ("terminal_range_getter", "generated_terminal_range_getter", "handwritten_terminal_range_getter", False, 8),
    ("terminal_range_builder", "generated_terminal_range_builder", "handwritten_terminal_range_builder", False, 4),
    ("terminal_existing_range_builder", "generated_terminal_existing_range_builder", "handwritten_terminal_existing_range_builder", False, 4),
)
ALLOWED_OVERHEAD = {
    # The generated prepared probe returns its observable fixed encoded length through
    # two tuple-construction instructions; LLVM propagates the handwritten constant
    # into callsites instead. Commit branches, stores, and calls remain identical.
    "prepared_builder": {"instructions": 2},
    # Safe exact slicing of private, prevalidated dynamic geometry retains one
    # bounds-check path across the prepared-plan handoff on the pinned toolchain.
    "dynamic_prepared_builder": {
        "instructions": 13,
        "calls": 1,
        "branches": 2,
        "panic_paths": 1,
    },
    "relative_range_mutation": {"instructions": 2},
    "absolute_range_getter": {"instructions": 2},
}
ALLOWED_EXTRA_CALLEE_MARKERS = {
    "dynamic_prepared_builder": ("slice_index_fail",),
}
PANIC_MARKERS = (
    "panic",
    "bounds_check",
    "slice_index",
    "copy_from_slice",
    "assert_failed",
)
FORBIDDEN_MARKERS = ("__rust_alloc", "__rust_realloc", "__rust_dealloc", "vtable")


def command(root: Path, target: str) -> None:
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(root / "target" / "codegen-gate")
    if sys.platform == "darwin" and target.endswith("-unknown-linux-gnu"):
        linker_variable = f"CARGO_TARGET_{target.upper().replace('-', '_')}_LINKER"
        # LLVM IR and assembly are emitted before Cargo's mandatory final link.
        # A Linux userspace is unavailable on macOS and the probe binary is never run.
        environment.setdefault(linker_variable, "/usr/bin/true")
    subprocess.run(
        [
            "cargo",
            "+1.91.0",
            "rustc",
            "-p",
            "wire-repr",
            "--test",
            "codegen",
            "--release",
            "--target",
            target,
            "--",
            "--emit=asm,llvm-ir",
        ],
        cwd=root,
        check=True,
        env=environment,
    )


def unquote(symbol: str) -> str:
    return symbol.strip('"')


def aliases(ir: str) -> dict[str, str]:
    result: dict[str, str] = {}
    for line in ir.splitlines():
        match = re.match(r'^@(?P<alias>"?[\w.$-]+"?)\s*=\s*alias\b.*?@(?P<target>"?[\w.$-]+"?)\s*$', line)
        if match:
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
    seen = set()
    while symbol in alias_map and symbol not in seen:
        seen.add(symbol)
        symbol = alias_map[symbol]
    return symbol


def locate(
    marker: str, bodies: dict[str, str], alias_map: dict[str, str]
) -> tuple[str, str] | None:
    candidates = [symbol for symbol in bodies if marker in symbol]
    candidates.extend(symbol for symbol in alias_map if marker in symbol)
    candidates = sorted(set(candidates))
    resolved = [(symbol, resolve(symbol, alias_map)) for symbol in candidates]
    available = [(symbol, target) for symbol, target in resolved if target in bodies]
    if not candidates:
        return None
    if len(available) != 1:
        names = ", ".join(f"{symbol}->{target}" for symbol, target in resolved)
        raise RuntimeError(f"expected one symbol containing {marker!r}; found {names}")
    symbol, target = available[0]
    return symbol, bodies[target]


def normalized(body: str) -> str:
    body = re.sub(r'\bnoundef\s+', '', body)
    body = re.sub(r', ![\w.]+ !\d+', '', body)
    body = re.sub(r'!dbg !\d+', '', body)
    body = re.sub(r'^\s*[\w.$-]+:', 'block:', body, flags=re.MULTILINE)
    body = re.sub(r'%[\w.]+', '%value', body)
    body = re.sub(r'\s+', ' ', body).strip()
    return body


def call_references(assembly: str, symbol: str) -> int:
    pattern = rf"\b(?:callq?|bl)\s+_?{re.escape(symbol)}(?:@PLT)?\b"
    return len(re.findall(pattern, assembly))


def external_callees(body: str) -> set[str]:
    return {
        match.group("callee")
        for match in re.finditer(r"\b(?:call|invoke)\b.*?@(?P<callee>\"?[\w.$-]+\"?)", body)
        if not unquote(match.group("callee")).startswith("llvm.")
    }


def metrics(body: str) -> dict[str, int]:
    lines = [line.strip() for line in body.splitlines() if line.strip() and not line.lstrip().startswith(";")]
    calls = [line for line in lines if re.search(r'\b(?:call|invoke)\b', line)]
    relevant_calls = [line for line in calls if "llvm." not in line]
    return {
        "instructions": sum(1 for line in lines if not line.endswith(":")),
        "calls": len(relevant_calls),
        "branches": sum(1 for line in lines if line.startswith("br ") or line.startswith("switch ")),
        "panic_paths": sum(1 for line in relevant_calls if any(marker in line for marker in PANIC_MARKERS)),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--target", default="x86_64-unknown-linux-gnu")
    args = parser.parse_args()
    root = args.root.resolve()
    command(root, args.target)

    deps = root / "target" / "codegen-gate" / args.target / "release" / "deps"
    ir_files = sorted(deps.glob("codegen-*.ll"), key=lambda path: path.stat().st_mtime)
    if not ir_files:
        raise RuntimeError(f"expected optimized codegen LLVM IR below {deps}")
    ir_path = ir_files[-1]
    asm_path = ir_path.with_suffix(".s")
    if not asm_path.is_file():
        raise RuntimeError(f"expected matching optimized assembly at {asm_path}")
    ir = ir_path.read_text()
    assembly = asm_path.read_text()
    body_map = definitions(ir)
    alias_map = aliases(ir)

    failures: list[str] = []
    for label, generated_marker, handwritten_marker, strict, merged_references in PAIRS:
        try:
            generated = locate(generated_marker, body_map, alias_map)
            handwritten = locate(handwritten_marker, body_map, alias_map)
        except RuntimeError as error:
            failures.append(f"{label}: {error}")
            continue
        if generated is None or handwritten is None:
            # A merged pair shares one optimized body, so every metric delta is zero.
            # Callsite coverage proves both probes exercise that surviving body.
            survivor = generated or handwritten
            if survivor is None:
                failures.append(f"{label}: neither probe symbol survived release optimization")
            else:
                survivor_symbol, survivor_body = survivor
                references = call_references(assembly, survivor_symbol)
                survivor_metrics = metrics(survivor_body)
                forbidden = [
                    marker for marker in FORBIDDEN_MARKERS if marker in survivor_body
                ]
                if forbidden:
                    failures.append(
                        f"{label}: merged body contains forbidden codegen markers: "
                        f"{forbidden}"
                    )
                if survivor_metrics["calls"] or survivor_metrics["panic_paths"]:
                    failures.append(
                        f"{label}: merged body contains unexpected external calls: "
                        f"{survivor_metrics}"
                    )
                if references != merged_references:
                    failures.append(
                        f"{label}: one probe symbol disappeared but the survivor has "
                        f"{references} assembly callsite reference(s), expected "
                        f"{merged_references}"
                    )
                elif not forbidden and not survivor_metrics["calls"]:
                    print(
                        f"{label}: optimizer merged the pair into {survivor_symbol} "
                        f"({references} assembly callsite references, "
                        f"metrics={survivor_metrics})"
                    )
            continue
        generated_symbol, generated_body = generated
        handwritten_symbol, handwritten_body = handwritten
        generated_metrics = metrics(generated_body)
        handwritten_metrics = metrics(handwritten_body)
        identical = normalized(generated_body) == normalized(handwritten_body)
        allowed_overhead = ALLOWED_OVERHEAD.get(label, {})
        extra = {
            key: generated_metrics[key] - handwritten_metrics[key]
            for key in generated_metrics
            if generated_metrics[key] - handwritten_metrics[key]
            > allowed_overhead.get(key, 0)
        }
        forbidden = [marker for marker in FORBIDDEN_MARKERS if marker in generated_body]
        extra_callees = external_callees(generated_body) - external_callees(handwritten_body)
        allowed_callee_markers = ALLOWED_EXTRA_CALLEE_MARKERS.get(label, ())
        allowed_callees = {
            callee
            for callee in extra_callees
            if any(marker in callee for marker in allowed_callee_markers)
        }
        unexpected_callees = extra_callees - allowed_callees
        print(
            f"{label}: generated={generated_symbol} handwritten={handwritten_symbol} "
            f"identical={identical} generated_metrics={generated_metrics} "
            f"handwritten_metrics={handwritten_metrics}"
        )
        if forbidden:
            failures.append(f"{label}: generated body contains forbidden codegen markers: {forbidden}")
        if unexpected_callees:
            failures.append(f"{label}: generated body has unexpected callees: {sorted(unexpected_callees)}")
        if len(allowed_callees) > len(allowed_callee_markers):
            failures.append(
                f"{label}: generated body has too many allowed extra callees: "
                f"{sorted(allowed_callees)}"
            )
        if strict and not identical:
            failures.append(f"{label}: generated and handwritten optimized bodies differ")
        elif not identical and extra:
            failures.append(f"{label}: generated overhead exceeds handwritten equivalent: {extra}")

    if failures:
        print("codegen regression gate failed:", file=sys.stderr)
        print("\n".join(f"  - {failure}" for failure in failures), file=sys.stderr)
        return 1
    print("codegen regression gate passed")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, subprocess.CalledProcessError, RuntimeError) as error:
        print(f"codegen regression gate failed: {error}", file=sys.stderr)
        raise SystemExit(1)
