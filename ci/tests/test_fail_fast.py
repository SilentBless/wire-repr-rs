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


class FailFastDiscoveryTests(unittest.TestCase):
    def test_discovery_recurses_and_case_names_include_directories(self):
        with tempfile.TemporaryDirectory() as directory:
            fixtures = Path(directory)
            nested = fixtures / "computed" / "cycle.rs"
            nested.parent.mkdir()
            nested.write_text("//! error: rejected\n")
            (fixtures / "root.rs").write_text("//! error: rejected\n")
            (fixtures / "computed" / "note.txt").write_text("not a fixture\n")

            discovered = FAIL_FAST.discover_fixtures(fixtures)

            self.assertEqual(
                [path.relative_to(fixtures).as_posix() for path in discovered],
                ["computed/cycle.rs", "root.rs"],
            )
            self.assertEqual(FAIL_FAST.case_name(nested, fixtures), "computed_cycle")

if __name__ == "__main__":
    unittest.main()
