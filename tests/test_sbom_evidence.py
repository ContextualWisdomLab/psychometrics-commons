"""Executable contract for least-privilege, exact-head SPDX SBOM evidence."""

from pathlib import Path
import unittest


WORKFLOW = Path(".github/workflows/sbom-evidence.yml")
VALIDATOR = Path("scripts/validate_spdx_sbom.py")


class SbomEvidenceContract(unittest.TestCase):
    """Keep SBOM generation immutable, review-safe, and tied to Cargo.lock."""

    @classmethod
    def workflow_text(cls) -> str:
        """Return the committed SBOM workflow text."""
        return WORKFLOW.read_text(encoding="utf-8")

    def test_generation_is_exact_head_and_immutably_pinned(self) -> None:
        """The workflow must scan the exact revision using fixed action/tool identities."""
        text = self.workflow_text()
        self.assertIn("pull_request:", text)
        self.assertNotIn("pull_request_target:", text)
        self.assertIn("github.event.pull_request.head.sha || github.sha", text)
        self.assertIn("persist-credentials: false", text)
        self.assertIn(
            "anchore/sbom-action@e22c389904149dbc22b58101806040fa8d37a610", text
        )
        self.assertIn("syft-version: v1.51.0", text)
        self.assertIn("format: spdx-json", text)

    def test_pull_request_lane_is_read_only_and_does_not_publish(self) -> None:
        """Untrusted pull-request code must not obtain release or dependency-write authority."""
        text = self.workflow_text()
        self.assertIn("contents: read", text)
        self.assertNotIn("contents: write", text)
        self.assertIn("dependency-snapshot: false", text)
        self.assertIn("upload-artifact: false", text)
        self.assertIn("upload-release-assets: false", text)

    def test_generated_sbom_is_validated_against_locked_rust_dependencies(self) -> None:
        """An uploaded file must be parseable SPDX evidence covering Cargo.lock dependencies."""
        text = self.workflow_text()
        self.assertTrue(VALIDATOR.is_file())
        self.assertIn(
            "python3 scripts/validate_spdx_sbom.py sbom.spdx.json Cargo.lock", text
        )
        self.assertIn("actions/upload-artifact@b7c566a772e6b6bfb58ed0dc250532a479d7789f", text)
        self.assertIn("sbom.spdx.json", text)
        self.assertIn("sbom-spdx-${{ github.event.pull_request.head.sha || github.sha }}", text)


if __name__ == "__main__":
    unittest.main()
