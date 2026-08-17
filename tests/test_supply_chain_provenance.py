"""Executable contract for exact-source packaging and protected-main provenance."""

from pathlib import Path
import unittest


WORKFLOW = Path(".github/workflows/supply-chain-provenance.yml")


def mapping_block(text: str, key: str, indent: int) -> str:
    """Return one indentation-delimited YAML mapping block without parsing untrusted tags."""
    lines = text.splitlines()
    marker = f"{' ' * indent}{key}:"
    try:
        start = lines.index(marker) + 1
    except ValueError as error:
        raise AssertionError(f"missing YAML mapping key {key!r} at indent {indent}") from error

    block: list[str] = []
    for line in lines[start:]:
        if not line.strip():
            block.append(line)
            continue
        current_indent = len(line) - len(line.lstrip(" "))
        if current_indent <= indent:
            break
        block.append(line)
    return "\n".join(block)


def mapping_scalar(text: str, key: str, indent: int) -> str:
    """Return one scalar value from an exact indentation level."""
    prefix = f"{' ' * indent}{key}:"
    for line in text.splitlines():
        if line.startswith(prefix):
            return line[len(prefix) :].strip()
    raise AssertionError(f"missing YAML scalar key {key!r} at indent {indent}")


class SupplyChainProvenanceContract(unittest.TestCase):
    """Keep package provenance fail-closed and independently reproducible."""

    @classmethod
    def workflow_text(cls) -> str:
        """Return the committed provenance workflow text."""
        return WORKFLOW.read_text(encoding="utf-8")

    def test_workflow_is_pull_request_safe_and_immutably_pinned(self) -> None:
        """Untrusted PR code must never receive attestation credentials."""
        text = self.workflow_text()
        trigger_block = mapping_block(text, "on", 0)
        self.assertIn("pull_request:", trigger_block)
        self.assertNotIn("pull_request_target:", trigger_block)
        self.assertNotIn(
            "paths:",
            trigger_block,
            "provenance must run for every repository change because Cargo's default package set is VCS-derived",
        )
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

    def test_concurrency_never_cancels_a_different_protected_main_revision(self) -> None:
        """A newer main push must not erase provenance execution for an older exact source SHA."""
        text = self.workflow_text()
        concurrency = mapping_block(text, "concurrency", 0)
        self.assertEqual(
            mapping_scalar(concurrency, "group", 2),
            "psychometrics-commons-provenance-${{ github.event.pull_request.number || github.sha }}",
        )
        self.assertEqual(mapping_scalar(concurrency, "cancel-in-progress", 2), "true")
        self.assertNotIn("github.ref", concurrency)

    def test_package_job_preserves_exact_source_and_checksum_evidence(self) -> None:
        """The source package must come from the exact checked-out revision."""
        text = self.workflow_text()
        package_job = mapping_block(mapping_block(text, "jobs", 0), "package", 2)
        self.assertIn("github.event.pull_request.head.sha || github.sha", package_job)
        self.assertIn("persist-credentials: false", package_job)
        self.assertIn("cargo package --locked", package_job)
        self.assertIn("sha256sum *.crate > SHA256SUMS", package_job)
        self.assertIn("sha256sum --check SHA256SUMS", package_job)
        self.assertIn("target/package/*.crate", package_job)

    def test_package_job_proves_binary_reproducibility_in_isolated_target_dirs(self) -> None:
        """Two isolated locked package runs must produce the same crate before attestation."""
        text = self.workflow_text()
        package_job = mapping_block(mapping_block(text, "jobs", 0), "package", 2)
        self.assertEqual(package_job.count("cargo package --locked --target-dir"), 2)
        self.assertIn("target/repro-one", package_job)
        self.assertIn("target/repro-two", package_job)
        self.assertIn('test "${#first_packages[@]}" -eq 1', package_job)
        self.assertIn('test "${#second_packages[@]}" -eq 1', package_job)
        self.assertIn('cmp "${first_packages[0]}" "${second_packages[0]}"', package_job)
        self.assertIn('cp "${first_packages[0]}" target/package/', package_job)

    def test_attestation_credentials_exist_only_on_protected_main_push(self) -> None:
        """OIDC and only the permissions required for binary attestation stay on protected main."""
        text = self.workflow_text()
        top_permissions = mapping_block(text, "permissions", 0)
        jobs = mapping_block(text, "jobs", 0)
        package_job = mapping_block(jobs, "package", 2)
        attest_job = mapping_block(jobs, "attest", 2)
        package_permissions = mapping_block(package_job, "permissions", 4)
        attest_permissions = mapping_block(attest_job, "permissions", 4)

        self.assertEqual(
            [line.strip() for line in top_permissions.splitlines() if line.strip()],
            ["contents: read"],
        )
        self.assertEqual(
            [line.strip() for line in package_permissions.splitlines() if line.strip()],
            ["contents: read"],
        )
        self.assertNotIn("id-token:", package_job)
        self.assertNotIn("attestations:", package_job)
        self.assertNotIn("artifact-metadata:", package_job)
        self.assertEqual(mapping_scalar(attest_job, "needs", 4), "package")
        self.assertEqual(
            set(line.strip() for line in attest_permissions.splitlines() if line.strip()),
            {
                "contents: read",
                "id-token: write",
                "attestations: write",
            },
        )
        self.assertNotIn("artifact-metadata:", attest_job)
        self.assertEqual(
            mapping_scalar(attest_job, "if", 4),
            "github.event_name == 'push' && github.ref == 'refs/heads/main'",
        )
        self.assertIn("subject-path: package/*.crate", attest_job)
        self.assertIn("python3 tests/test_supply_chain_provenance.py", package_job)

    def test_protected_main_attestation_is_verified_against_exact_signer_and_source(self) -> None:
        """A stored provenance claim must verify against this workflow and exact main SHA."""
        text = self.workflow_text()
        attest_job = mapping_block(mapping_block(text, "jobs", 0), "attest", 2)
        self.assertIn('GH_TOKEN: ${{ github.token }}', attest_job)
        self.assertIn('gh attestation verify "$package_file"', attest_job)
        self.assertIn('--repo "$GITHUB_REPOSITORY"', attest_job)
        self.assertIn(
            '--signer-workflow "$GITHUB_REPOSITORY/.github/workflows/supply-chain-provenance.yml"',
            attest_job,
        )
        self.assertIn('--source-ref "refs/heads/main"', attest_job)
        self.assertIn('--source-digest "$GITHUB_SHA"', attest_job)


if __name__ == "__main__":
    unittest.main()
