"""Executable contract for exact-source packaging and protected-main provenance."""

from pathlib import Path
import unittest


WORKFLOW = Path(".github/workflows/supply-chain-provenance.yml")


class SupplyChainProvenanceContract(unittest.TestCase):
    """Keep package provenance fail-closed and independently reproducible."""

    @classmethod
    def workflow_text(cls) -> str:
        """Return the committed provenance workflow text."""
        return WORKFLOW.read_text(encoding="utf-8")

    def test_workflow_is_pull_request_safe_and_immutably_pinned(self) -> None:
        """Untrusted PR code must never receive attestation credentials."""
        text = self.workflow_text()
        self.assertIn("pull_request:", text)
        self.assertNotIn("pull_request_target:", text)
        self.assertIn(
            "actions/checkout@631c942040754b6e095e929c1677c07e10ed4f87", text
        )
        self.assertIn(
            "actions/upload-artifact@b7c566a772e6b6bfb58ed0dc250532a479d7789f", text
        )
        self.assertIn(
            "actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c", text
        )
        self.assertIn(
            "actions/attest@508db95dd578ae2727ebd6217d5ba78e4fbda05d", text
        )

    def test_package_job_preserves_exact_source_and_checksum_evidence(self) -> None:
        """The source package must come from the exact checked-out revision."""
        text = self.workflow_text()
        self.assertIn("github.event.pull_request.head.sha || github.sha", text)
        self.assertIn("persist-credentials: false", text)
        self.assertIn("cargo package --locked", text)
        self.assertIn("sha256sum *.crate > SHA256SUMS", text)
        self.assertIn("sha256sum --check SHA256SUMS", text)
        self.assertIn("target/package/*.crate", text)

    def test_attestation_credentials_exist_only_on_protected_main_push(self) -> None:
        """OIDC and attestation writes must be unreachable from pull requests."""
        text = self.workflow_text()
        self.assertIn(
            "if: github.event_name == 'push' && github.ref == 'refs/heads/main'", text
        )
        self.assertIn("id-token: write", text)
        self.assertIn("attestations: write", text)
        self.assertIn("artifact-metadata: write", text)
        self.assertIn("subject-path: package/*.crate", text)
        self.assertIn("python3 tests/test_supply_chain_provenance.py", text)

    def test_protected_main_attestation_is_verified_against_exact_signer_and_source(self) -> None:
        """A stored provenance claim must verify against this workflow and exact main SHA."""
        text = self.workflow_text()
        self.assertIn('GH_TOKEN: ${{ github.token }}', text)
        self.assertIn('gh attestation verify "$package_file"', text)
        self.assertIn('--repo "$GITHUB_REPOSITORY"', text)
        self.assertIn(
            '--signer-workflow "$GITHUB_REPOSITORY/.github/workflows/supply-chain-provenance.yml"',
            text,
        )
        self.assertIn('--source-ref "refs/heads/main"', text)
        self.assertIn('--source-digest "$GITHUB_SHA"', text)


if __name__ == "__main__":
    unittest.main()
