"""Regression tests for the fail-closed LLVM production-coverage contract."""

from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).parents[1] / "scripts" / "check_coverage.py"
SPEC = importlib.util.spec_from_file_location("check_coverage", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
CHECK_COVERAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK_COVERAGE)


class CoverageContractTests(unittest.TestCase):
    """Verify production coverage is exact and cannot be forged."""

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

    def test_test_harness_gaps_do_not_dilute_production_coverage(self) -> None:
        """Coverage enforcement must measure owned Rust production sources only."""
        payload = {
            "data": [
                {
                    "totals": {
                        "lines": {"count": 10, "covered": 9},
                        "branches": {"count": 4, "covered": 3},
                    },
                    "files": [
                        {
                            "filename": "/workspace/psychometrics-commons/src/runtime.rs",
                            "summary": {
                                "lines": {"count": 7, "covered": 7},
                                "branches": {"count": 2, "covered": 2},
                            },
                        },
                        {
                            "filename": "/workspace/psychometrics-commons/tests/runtime_contract.rs",
                            "summary": {
                                "lines": {"count": 3, "covered": 2},
                                "branches": {"count": 2, "covered": 1},
                            },
                        },
                    ],
                }
            ]
        }
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "coverage.json"
            report.write_text(json.dumps(payload), encoding="utf-8")
            self.assertEqual(
                CHECK_COVERAGE.validate_report(report, ("lines", "branches")),
                [
                    "lines coverage: PASS (7/7, 100%)",
                    "branches coverage: PASS (2/2, 100%)",
                ],
            )

    def test_uncovered_production_source_still_fails_closed(self) -> None:
        """Ignoring test-harness files must never hide a production coverage gap."""
        payload = {
            "data": [
                {
                    "totals": {"lines": {"count": 9, "covered": 8}},
                    "files": [
                        {
                            "filename": "/workspace/psychometrics-commons/src/runtime.rs",
                            "summary": {"lines": {"count": 8, "covered": 7}},
                        },
                        {
                            "filename": "/workspace/psychometrics-commons/tests/runtime_contract.rs",
                            "summary": {"lines": {"count": 1, "covered": 1}},
                        },
                    ],
                }
            ]
        }
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "coverage.json"
            report.write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaisesRegex(
                ValueError, "lines coverage is incomplete: 7/8"
            ):
                CHECK_COVERAGE.validate_report(report, ("lines",))


if __name__ == "__main__":
    unittest.main()
