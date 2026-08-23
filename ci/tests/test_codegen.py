import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("check_codegen", ROOT / "ci" / "check-codegen.py")
assert SPEC is not None and SPEC.loader is not None
CODEGEN = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = CODEGEN
SPEC.loader.exec_module(CODEGEN)


class CodegenMetadataTests(unittest.TestCase):
    @staticmethod
    def fixtures(mode, identities):
        pairs = tuple(
            CODEGEN.Pair(fixture, name, f"generated_{index}", f"handwritten_{index}")
            for index, identity in enumerate(identities)
            for fixture, name in [identity.split("/", 1)]
        )
        return (CODEGEN.Fixture(Path("fixture.rs"), pairs, None, {}, mode),)

    def test_percentage_requires_explicit_finite_nonnegative_percent(self):
        self.assertEqual(CODEGEN.percentage("10%", "test"), 0.1)
        for value in ["10", "-1%", "nan%", "inf%"]:
            with self.subTest(value=value), self.assertRaises(RuntimeError):
                CODEGEN.percentage(value, "test")

    def test_weights_reject_unknown_duplicate_and_empty_effective_weights(self):
        self.assertEqual(
            CODEGEN.weight_map("instructions=1, branches=4", "test"),
            {"instructions": 1.0, "branches": 4.0},
        )
        for value in ["unknown=1", "calls=1,calls=2", "calls=-1", "calls=0"]:
            with self.subTest(value=value), self.assertRaises(RuntimeError):
                CODEGEN.weight_map(value, "test")

    def test_fixture_requires_pairs_and_rejects_unknown_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "fixture.rs"
            path.write_text("//! tolerance: 10%\n")
            with self.assertRaises(RuntimeError):
                CODEGEN.parse_fixture(root, path)
            path.write_text("//! mystery: value\n//! pair: x = generated / handwritten\n")
            with self.assertRaises(RuntimeError):
                CODEGEN.parse_fixture(root, path)

    def test_fixture_mode_is_explicit_and_unique(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "fixture.rs"
            path.write_text("//! mode: bytes\n//! pair: x = generated / handwritten\n")
            self.assertEqual(CODEGEN.parse_fixture(root, path).mode, "bytes")
            path.write_text(
                "//! mode: bytes\n"
                "//! mode: default\n"
                "//! pair: x = generated / handwritten\n"
            )
            with self.assertRaises(RuntimeError):
                CODEGEN.parse_fixture(root, path)

    def test_parse_fixture_derives_name_from_nested_temp_root(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "owned" / "append.rs"
            path.parent.mkdir()
            path.write_text("//! pair: dynamic = generated / handwritten\n")
            fixture = CODEGEN.parse_fixture(root, path)
            self.assertEqual(fixture.pairs[0].identity, "owned_append/dynamic")

    def test_discover_recurses_into_nested_fixture_directories(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "wire-repr" / "tests" / "fixtures" / "owned" / "append.rs"
            path.parent.mkdir(parents=True)
            path.write_text("//! pair: dynamic = generated / handwritten\n")
            with patch.dict(CODEGEN.REQUIRED_PAIRS, {"default": frozenset({"owned_append/dynamic"})}, clear=True):
                fixtures = CODEGEN.discover(root, "default")
            self.assertEqual(len(fixtures), 1)
            self.assertEqual(fixtures[0].pairs[0].identity, "owned_append/dynamic")

    def test_required_pairs_accept_complete_inventory(self):
        for mode, identities in CODEGEN.REQUIRED_PAIRS.items():
            with self.subTest(mode=mode):
                CODEGEN.require_pairs(self.fixtures(mode, identities), mode)

    def test_required_pairs_reports_missing_identity(self):
        mode = "default"
        identity = "complex_computed/dependency"
        identities = CODEGEN.REQUIRED_PAIRS[mode] - {identity}
        with self.assertRaisesRegex(RuntimeError, "default.*complex_computed/dependency"):
            CODEGEN.require_pairs(self.fixtures(mode, identities), mode)

    def test_required_pairs_permits_extra_identities(self):
        mode = "bytes"
        identities = CODEGEN.REQUIRED_PAIRS[mode] | {"new_fixture/new_pair"}
        CODEGEN.require_pairs(self.fixtures(mode, identities), mode)


class CodegenStackTests(unittest.TestCase):
    def test_assembly_instructions_ignores_labels_directives_blanks_and_comments(self):
        body = """\
    .cfi_startproc
.Lentry:
    # setup
    pushq %rbp # preserve frame pointer
1:
    movq %rsp, %rbp
    .cfi_def_cfa_register %rbp
    jne .Lentry
    retq
    ; done

"""
        self.assertEqual(CODEGEN.assembly_instructions(body), 4)

    def test_stack_bytes_uses_maximum_cfa_offset_across_prologue_and_restore(self):
        assembly = """\
example:
    .cfi_startproc
    pushq %rbp
    .cfi_def_cfa_offset 16
    pushq %rbx
    .cfi_def_cfa_offset 24
    subq $144, %rsp
    .cfi_def_cfa_offset 168
    addq $144, %rsp
    .cfi_def_cfa_offset 24
    popq %rbx
    .cfi_def_cfa_offset 16
    retq
    .cfi_endproc
next:
    retq
"""
        self.assertEqual(CODEGEN.stack_bytes(assembly, "example", CODEGEN.STACK_TARGET), 160)

    def test_stack_bytes_is_zero_for_found_leaf_without_cfi(self):
        assembly = """\
leaf:
    xorl %eax, %eax
    retq
other:
    retq
"""
        self.assertEqual(CODEGEN.stack_bytes(assembly, "leaf", CODEGEN.STACK_TARGET), 0)

    def test_stack_gate_rejects_generated_stack_when_handwritten_is_zero(self):
        passes, overhead, limit = CODEGEN.stack_within_tolerance(168, 0, 0.10)
        self.assertFalse(passes)
        self.assertEqual(overhead, float("inf"))
        self.assertEqual(limit, 0)

    def test_stack_gate_rejects_stack_over_tolerance(self):
        passes, overhead, limit = CODEGEN.stack_within_tolerance(104, 56, 0.10)
        self.assertFalse(passes)
        self.assertGreater(overhead, 0.10)
        self.assertAlmostEqual(limit, 61.6)

    def test_stack_gate_accepts_equal_stack(self):
        passes, overhead, limit = CODEGEN.stack_within_tolerance(24, 24, 0.10)
        self.assertTrue(passes)
        self.assertEqual(overhead, 0)
        self.assertAlmostEqual(limit, 26.4)

    def test_matching_assembly_requires_the_same_stem_as_llvm_ir(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            ir_path = directory / "codegen_harness-new.ll"
            ir_path.write_text("ir")
            (directory / "codegen_harness-old.s").write_text("unrelated assembly")
            with self.assertRaisesRegex(RuntimeError, "codegen_harness-new.s"):
                CODEGEN.matching_assembly(ir_path)
            expected = directory / "codegen_harness-new.s"
            expected.write_text("matching assembly")
            self.assertEqual(CODEGEN.matching_assembly(ir_path), expected)


if __name__ == "__main__":
    unittest.main()
