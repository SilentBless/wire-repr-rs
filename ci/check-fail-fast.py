#!/usr/bin/env python3
"""Compile every fail-fast fixture and verify its declared diagnostics."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ERROR = re.compile(r"^//! error:\s*(?P<fragment>\S(?:.*\S)?)\s*$")
METADATA = re.compile(r"^//!\s*(?P<key>[A-Za-z][A-Za-z0-9_-]*):")


def discover_fixtures(directory: Path) -> list[Path]:
    return sorted(path for path in directory.rglob("*.rs") if path.is_file())


def case_name(path: Path, directory: Path) -> str:
    return "_".join(path.relative_to(directory).with_suffix("").parts)


def expected_errors(path: Path) -> tuple[str, ...]:
    fragments: list[str] = []
    for line_number, line in enumerate(path.read_text().splitlines(), 1):
        match = ERROR.match(line)
        if match:
            fragment = match.group("fragment")
            if fragment in fragments:
                raise RuntimeError(f"{path}:{line_number}: duplicate error fragment {fragment!r}")
            fragments.append(fragment)
            continue
        metadata = METADATA.match(line)
        if metadata:
            raise RuntimeError(
                f"{path}:{line_number}: unknown metadata key {metadata.group('key')!r}"
            )
    if not fragments:
        raise RuntimeError(f"{path}: expected at least one `//! error:` fragment")
    return tuple(fragments)


def diagnostic_messages(value: dict[str, object]) -> list[str]:
    messages: list[str] = []
    message = value.get("message")
    if isinstance(message, str):
        messages.append(message)
    children = value.get("children")
    if isinstance(children, list):
        for child in children:
            if isinstance(child, dict):
                messages.extend(diagnostic_messages(child))
    return messages


def compile_case(root: Path, path: Path, directory: Path, toolchain: str) -> list[str]:
    name = case_name(path, directory)
    case_root = root / "target" / "fail-fast" / "cases" / name
    source_root = case_root / "src"
    source_root.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(path, source_root / "lib.rs")
    (case_root / "Cargo.toml").write_text(
        "\n".join(
            [
                "[package]",
                f'name = "wire-repr-fail-{name.replace("_", "-")}"',
                'version = "0.0.0"',
                'edition = "2024"',
                "",
                "[dependencies]",
                'thiserror = { version = "2", default-features = false }',
                f'wire-repr = {{ path = {json.dumps(str(root / "wire-repr"))} }}',
                "",
                "[workspace]",
                "",
            ]
        )
    )
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(root / "target" / "fail-fast" / "build")
    result = subprocess.run(
        [
            "cargo",
            f"+{toolchain}",
            "check",
            "--manifest-path",
            str(case_root / "Cargo.toml"),
            "--message-format=json",
        ],
        cwd=root,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode == 0:
        raise RuntimeError(f"{path}: compiled successfully but failure was required")

    messages: list[str] = []
    for line in result.stdout.splitlines():
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if value.get("reason") != "compiler-message":
            continue
        diagnostic = value.get("message")
        if isinstance(diagnostic, dict) and diagnostic.get("level") == "error":
            messages.extend(diagnostic_messages(diagnostic))
    if not messages:
        raise RuntimeError(
            f"{path}: compilation failed without structured compiler errors\n{result.stderr}"
        )
    return messages


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--toolchain", default="1.91.0")
    args = parser.parse_args()
    root = args.root.resolve()
    directory = root / "wire-repr" / "tests" / "fail_fast"
    fixtures = discover_fixtures(directory)
    if not fixtures:
        raise RuntimeError(f"no fail-fast fixtures found below {directory}")

    failures: list[str] = []
    for path in fixtures:
        expected = expected_errors(path)
        messages = compile_case(root, path, directory, args.toolchain)
        missing = [fragment for fragment in expected if not any(fragment in message for message in messages)]
        relative = path.relative_to(root)
        if missing:
            failures.append(
                f"{relative}: missing diagnostic fragment(s) {missing!r}; "
                f"compiler messages were {messages!r}"
            )
        else:
            print(f"{relative}: rejected as expected ({', '.join(expected)})")

    if failures:
        print("fail-fast fixture gate failed:", file=sys.stderr)
        print("\n".join(f"  - {failure}" for failure in failures), file=sys.stderr)
        return 1
    print(f"fail-fast fixture gate passed ({len(fixtures)} cases)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError) as error:
        print(f"fail-fast fixture gate failed: {error}", file=sys.stderr)
        raise SystemExit(1)
