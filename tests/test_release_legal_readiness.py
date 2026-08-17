"""Executable contract for fail-closed source-distribution legal readiness."""

from pathlib import Path
import tempfile
import unittest


SCRIPT = Path("scripts/check_release_legal_readiness.py")
WORKFLOW = Path(".github/workflows/release-legal-readiness.yml")
RUNTIME_CI = Path(".github/workflows/ci.yml")


class ReleaseLegalReadinessContract(unittest.TestCase):
    """Require explicit license evidence without inventing a license choice."""

    def test_repository_owns_a_deterministic_readiness_checker(self) -> None:
        """The preflight checker must exist before any release can claim legal readiness."""
        self.assertTrue(SCRIPT.is_file())
        source = SCRIPT.read_text(encoding="utf-8")
        self.assertIn("LICENSE_CANDIDATES", source)
        self.assertIn("license-file", source)
        self.assertIn("license", source)
        self.assertIn("ReleaseLegalReadinessError", source)

    def test_preflight_is_manual_fail_closed_and_read_only(self) -> None:
        """A manual operator gate must not publish, mutate, or silently waive missing evidence."""
        self.assertTrue(WORKFLOW.is_file())
        text = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("workflow_dispatch:", text)
        self.assertNotIn("pull_request:", text)
        self.assertNotIn("release:", text)
        self.assertIn("contents: read", text)
        self.assertNotIn("contents: write", text)
        self.assertNotIn("id-token: write", text)
        self.assertIn("persist-credentials: false", text)
        self.assertIn('test "$GITHUB_REF" = "refs/heads/main"', text)
        self.assertIn("python3 tests/test_release_legal_readiness.py", text)
        self.assertIn("python3 tests/test_check_release_legal_readiness.py", text)
        self.assertIn("python3 scripts/check_release_legal_readiness.py .", text)

    def test_runtime_ci_cannot_skip_changes_to_release_preflight_sources(self) -> None:
        """Changing the checker, workflow, or root license evidence must trigger the contract suite."""
        self.assertTrue(RUNTIME_CI.is_file())
        text = RUNTIME_CI.read_text(encoding="utf-8")
        self.assertEqual(
            text.count('      - "scripts/check_release_legal_readiness.py"'),
            2,
            "pull-request and protected-main path filters must both include the checker",
        )
        self.assertEqual(
            text.count('      - ".github/workflows/release-legal-readiness.yml"'),
            2,
            "pull-request and protected-main path filters must both include the manual preflight workflow",
        )
        self.assertEqual(
            text.count('      - "LICENSE*"'),
            2,
            "pull-request and protected-main path filters must both include root LICENSE evidence",
        )
        self.assertEqual(
            text.count('      - "COPYING*"'),
            2,
            "pull-request and protected-main path filters must both include root COPYING evidence",
        )
        self.assertIn("python3 -m unittest discover -s tests -p 'test_*.py' -v", text)

    def test_current_repository_is_not_silently_declared_ready(self) -> None:
        """Until owners choose license terms, tests must not fabricate root license evidence."""
        license_candidates = [
            "LICENSE",
            "LICENSE.md",
            "LICENSE.txt",
            "COPYING",
            "COPYING.md",
            "COPYING.txt",
        ]
        self.assertFalse(
            any(Path(candidate).is_file() for candidate in license_candidates),
            "remove this assertion only in the same change that adds reviewed license evidence",
        )

    def test_fixture_directory_can_be_created_without_touching_repository(self) -> None:
        """Unit tests for the checker must use isolated fixtures rather than changing root rights evidence."""
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            (root / "Cargo.toml").write_text("[package]\nname = \"fixture\"\n", encoding="utf-8")
            self.assertTrue((root / "Cargo.toml").is_file())


if __name__ == "__main__":
    unittest.main()
