import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("check_codegen", ROOT / "ci" / "check-codegen.py")
assert SPEC is not None and SPEC.loader is not None
CODEGEN = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = CODEGEN
SPEC.loader.exec_module(CODEGEN)


class CodegenMetadataTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
