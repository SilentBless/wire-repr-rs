import importlib.util
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("check_fail_fast", ROOT / "ci" / "check-fail-fast.py")
assert SPEC is not None and SPEC.loader is not None
FAIL_FAST = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FAIL_FAST)


class FailFastMetadataTests(unittest.TestCase):
    def test_errors_are_mandatory_unique_and_strictly_named(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "case.rs"
            path.write_text("fn valid() {}\n")
            with self.assertRaises(RuntimeError):
                FAIL_FAST.expected_errors(path)
            path.write_text("//! error: rejected\n//! error: rejected\n")
            with self.assertRaises(RuntimeError):
                FAIL_FAST.expected_errors(path)
            path.write_text("//! diagnostic: rejected\n")
            with self.assertRaises(RuntimeError):
                FAIL_FAST.expected_errors(path)
            path.write_text("//! error: first\n//! error: second\n")
            self.assertEqual(FAIL_FAST.expected_errors(path), ("first", "second"))

    def test_diagnostic_messages_include_nested_reasons(self):
        diagnostic = {
            "message": "outer",
            "children": [{"message": "inner", "children": []}],
        }
        self.assertEqual(FAIL_FAST.diagnostic_messages(diagnostic), ["outer", "inner"])


if __name__ == "__main__":
    unittest.main()
