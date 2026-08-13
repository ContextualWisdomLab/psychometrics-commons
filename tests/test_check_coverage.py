"""Regression tests for the fail-closed LLVM coverage contract."""

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).parents[1] / "scripts" / "check_coverage.py"
SPEC = importlib.util.spec_from_file_location("check_coverage", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
CHECK_COVERAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK_COVERAGE)


class CoverageContractTests(unittest.TestCase):
    """Verify numeric coverage totals cannot be forged by JSON booleans."""

    def test_boolean_coverage_counts_fail_closed(self) -> None:
        """Reject bool even though Python exposes it as an int subclass."""
        for count, covered in ((True, True), (False, False), (1, True), (True, 1)):
            with self.subTest(count=count, covered=covered):
                with self.assertRaisesRegex(
                    ValueError, "lines count and covered values must be integers"
                ):
                    CHECK_COVERAGE.validate_kind(
                        {"lines": {"count": count, "covered": covered}}, "lines"
                    )

    def test_real_integer_coverage_counts_still_pass(self) -> None:
        """Preserve the success contract for exact integer totals."""
        self.assertEqual(
            CHECK_COVERAGE.validate_kind(
                {"lines": {"count": 7, "covered": 7}}, "lines"
            ),
            "lines coverage: PASS (7/7, 100%)",
        )


if __name__ == "__main__":
    unittest.main()
