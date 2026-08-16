"""Unit tests for source-distribution legal readiness evidence."""

from pathlib import Path
import sys
import tempfile
import unittest


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_release_legal_readiness import (  # noqa: E402
    ReleaseLegalReadinessError,
    evaluate_repository,
)


class ReleaseLegalReadinessTests(unittest.TestCase):
    """Prove that evidence is explicit and blockers fail closed."""

    def fixture_root(self, directory: str, package_lines: list[str]) -> Path:
        """Create one isolated Cargo package fixture."""
        root = Path(directory)
        (root / "Cargo.toml").write_text(
            "\n".join(["[package]", 'name = "fixture"', 'version = "0.1.0"', *package_lines])
            + "\n",
            encoding="utf-8",
        )
        return root

    def test_missing_license_file_and_manifest_metadata_are_blockers(self) -> None:
        """An owner decision cannot be replaced by an optimistic default."""
        with tempfile.TemporaryDirectory() as directory:
            evidence = evaluate_repository(self.fixture_root(directory, []))
            self.assertFalse(evidence["ready"])
            blockers = evidence["blockers"]
            self.assertIsInstance(blockers, list)
            self.assertEqual(len(blockers), 2)
            self.assertIn("no discoverable license file", blockers[0])
            self.assertIn("neither license nor license-file", blockers[1])

    def test_reviewed_root_file_plus_manifest_expression_is_ready_for_presence_gate(self) -> None:
        """Presence readiness succeeds without claiming the chosen terms are legally sufficient."""
        with tempfile.TemporaryDirectory() as directory:
            root = self.fixture_root(directory, ['license = "LicenseRef-Reviewed-Terms"'])
            (root / "LICENSE").write_text("reviewed fixture terms\n", encoding="utf-8")
            evidence = evaluate_repository(root)
            self.assertTrue(evidence["ready"])
            self.assertEqual(evidence["standard_license_files"], ["LICENSE"])
            self.assertTrue(evidence["cargo_license_expression_declared"])
            self.assertTrue(evidence["limitations"])

    def test_empty_or_whitespace_license_files_are_not_terms_evidence(self) -> None:
        """An empty conventional or declared file cannot satisfy the source-terms presence gate."""
        with tempfile.TemporaryDirectory() as directory:
            root = self.fixture_root(directory, ['license = "LicenseRef-Reviewed-Terms"'])
            (root / "LICENSE").write_text(" \n\t", encoding="utf-8")
            evidence = evaluate_repository(root)
            self.assertFalse(evidence["ready"])
            self.assertEqual(evidence["standard_license_files"], [])
            self.assertIn("no discoverable license file", evidence["blockers"][0])

        with tempfile.TemporaryDirectory() as directory:
            root = self.fixture_root(directory, ['license-file = "legal/source-license.txt"'])
            (root / "legal").mkdir()
            (root / "legal/source-license.txt").write_bytes(b"")
            evidence = evaluate_repository(root)
            self.assertFalse(evidence["ready"])
            self.assertIsNone(evidence["cargo_license_file_resolved"])
            self.assertIn("license-file is missing", evidence["blockers"][-1])

    def test_declared_license_file_must_exist_inside_repository(self) -> None:
        """Missing or path-escaping license-file metadata fails closed."""
        with tempfile.TemporaryDirectory() as directory:
            root = self.fixture_root(directory, ['license-file = "../LICENSE"'])
            evidence = evaluate_repository(root)
            self.assertFalse(evidence["ready"])
            self.assertIn("escapes the repository root", evidence["blockers"][-1])

    def test_standard_license_symlink_cannot_escape_repository(self) -> None:
        """A conventional filename is not evidence when its resolved file is outside the source tree."""
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as outside:
            root = self.fixture_root(directory, ['license = "LicenseRef-Reviewed-Terms"'])
            outside_license = Path(outside) / "LICENSE"
            outside_license.write_text("terms outside the repository\n", encoding="utf-8")
            (root / "LICENSE").symlink_to(outside_license)

            evidence = evaluate_repository(root)

            self.assertFalse(evidence["ready"])
            self.assertEqual(evidence["standard_license_files"], [])
            self.assertIn("no discoverable license file", evidence["blockers"][0])

    def test_declared_nonstandard_license_file_can_supply_explicit_file_evidence(self) -> None:
        """Cargo's reviewed license-file is acceptable even when its name is nonstandard."""
        with tempfile.TemporaryDirectory() as directory:
            root = self.fixture_root(directory, ['license-file = "legal/source-license.txt"'])
            (root / "legal").mkdir()
            (root / "legal/source-license.txt").write_text("reviewed fixture terms\n", encoding="utf-8")
            evidence = evaluate_repository(root)
            self.assertTrue(evidence["ready"])
            self.assertEqual(evidence["cargo_license_file_resolved"], "legal/source-license.txt")

    def test_malformed_manifest_is_an_inspection_error_not_ready_evidence(self) -> None:
        """Malformed release metadata cannot be treated as an ordinary missing field."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text("not = [valid", encoding="utf-8")
            with self.assertRaises(ReleaseLegalReadinessError):
                evaluate_repository(root)


if __name__ == "__main__":
    unittest.main()
