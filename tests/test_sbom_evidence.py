"""Executable contract for least-privilege, exact-head SPDX SBOM evidence."""

from pathlib import Path
import unittest


WORKFLOW = Path(".github/workflows/sbom-evidence.yml")
VALIDATOR = Path("scripts/validate_spdx_sbom.py")


def mapping_block(text: str, key: str, indent: int) -> str:
    """Return one indentation-delimited YAML mapping block without loading YAML tags."""
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


def named_step_block(text: str, name: str, indent: int) -> str:
    """Return one named YAML list step without allowing adjacent steps to satisfy it."""
    lines = text.splitlines()
    marker = f"{' ' * indent}- name: {name}"
    try:
        start = lines.index(marker)
    except ValueError as error:
        raise AssertionError(f"missing YAML step {name!r} at indent {indent}") from error

    block = [lines[start]]
    step_prefix = f"{' ' * indent}- "
    for line in lines[start + 1 :]:
        if line.startswith(step_prefix):
            break
        if line.strip():
            current_indent = len(line) - len(line.lstrip(" "))
            if current_indent < indent:
                break
        block.append(line)
    return "\n".join(block)


class SbomEvidenceContract(unittest.TestCase):
    """Keep SBOM generation immutable, review-safe, and tied to Cargo.lock."""

    @classmethod
    def workflow_text(cls) -> str:
        """Return the committed SBOM workflow text."""
        return WORKFLOW.read_text(encoding="utf-8")

    def test_generation_is_exact_head_and_immutably_pinned(self) -> None:
        """The workflow must scan every exact revision using fixed action/tool identities."""
        text = self.workflow_text()
        trigger_block = mapping_block(text, "on", 0)
        self.assertIn("pull_request:", trigger_block)
        self.assertNotIn("pull_request_target:", trigger_block)
        self.assertNotIn(
            "paths:",
            trigger_block,
            "root-directory SBOM evidence must run on every repository change",
        )
        self.assertIn("branches: [main]", trigger_block)
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
        top_level_permissions = mapping_block(text, "permissions", 0)
        self.assertEqual(
            [line.strip() for line in top_level_permissions.splitlines() if line.strip()],
            ["contents: read"],
            "workflow token authority must fail closed at the top level",
        )
        jobs = mapping_block(text, "jobs", 0)
        generate_job = mapping_block(jobs, "generate", 2)
        permissions = mapping_block(generate_job, "permissions", 4)
        self.assertEqual(
            [line.strip() for line in permissions.splitlines() if line.strip()],
            ["contents: read"],
        )
        for forbidden_permission in [
            "contents: write",
            "id-token:",
            "attestations:",
            "artifact-metadata:",
            "packages: write",
        ]:
            self.assertNotIn(forbidden_permission, generate_job)
        self.assertNotIn(
            "  verify-evidence:",
            jobs,
            "artifact reverification must not require a second hosted runner allocation",
        )
        self.assertIn("dependency-snapshot: false", generate_job)
        self.assertIn("upload-artifact: false", generate_job)
        self.assertIn("upload-release-assets: false", generate_job)

    def test_generated_sbom_is_validated_against_locked_rust_dependencies(self) -> None:
        """An uploaded file must be parseable SPDX evidence covering Cargo.lock dependencies."""
        text = self.workflow_text()
        generate_job = mapping_block(mapping_block(text, "jobs", 0), "generate", 2)
        self.assertTrue(VALIDATOR.is_file())
        self.assertIn(
            "python3 scripts/validate_spdx_sbom.py sbom.spdx.json Cargo.lock", generate_job
        )
        self.assertIn(
            "actions/upload-artifact@b7c566a772e6b6bfb58ed0dc250532a479d7789f",
            generate_job,
        )
        self.assertIn("sbom.spdx.json", generate_job)
        self.assertIn(
            "sbom-spdx-${{ github.event.pull_request.head.sha || github.sha }}", generate_job
        )

    def test_preserved_sbom_is_reverified_after_artifact_handoff(self) -> None:
        """The exact-head artifact-service copy must be downloaded and reverified in order."""
        text = self.workflow_text()
        generate_job = mapping_block(mapping_block(text, "jobs", 0), "generate", 2)
        upload = "actions/upload-artifact@b7c566a772e6b6bfb58ed0dc250532a479d7789f"
        download_action = (
            "actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c"
        )
        download_step = named_step_block(
            generate_job, "Download exact SBOM evidence", 6
        )
        checksum_step = named_step_block(
            generate_job, "Verify checksum after artifact handoff", 6
        )
        locked_dependency_step = named_step_block(
            generate_job,
            "Reverify locked dependency coverage after artifact handoff",
            6,
        )

        self.assertIn(download_action, download_step)
        self.assertIn(
            "name: sbom-spdx-${{ github.event.pull_request.head.sha || github.sha }}",
            download_step,
        )
        self.assertIn("path: evidence", download_step)
        self.assertIn("working-directory: evidence", checksum_step)
        self.assertIn("run: sha256sum --check sbom.spdx.json.sha256", checksum_step)
        self.assertIn(
            "run: python3 scripts/validate_spdx_sbom.py evidence/sbom.spdx.json Cargo.lock",
            locked_dependency_step,
        )

        upload_index = generate_job.index(upload)
        download_index = generate_job.index("- name: Download exact SBOM evidence")
        checksum_index = generate_job.index("- name: Verify checksum after artifact handoff")
        validation_index = generate_job.index(
            "- name: Reverify locked dependency coverage after artifact handoff"
        )
        self.assertLess(upload_index, download_index)
        self.assertLess(download_index, checksum_index)
        self.assertLess(checksum_index, validation_index)


if __name__ == "__main__":
    unittest.main()
