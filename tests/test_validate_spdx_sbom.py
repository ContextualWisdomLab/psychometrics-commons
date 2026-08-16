"""Unit tests for the deterministic SPDX/Cargo.lock evidence validator."""

from __future__ import annotations

import json
from pathlib import Path
import sys
import tempfile
import unittest


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from validate_spdx_sbom import SbomValidationError, validate_sbom  # noqa: E402


class ValidateSpdxSbomTests(unittest.TestCase):
    """Exercise successful coverage and fail-closed evidence mismatches."""

    def write_fixture(
        self, directory: Path, *, packages: list[dict[str, str]], spdx_version: str = "SPDX-2.3"
    ) -> tuple[Path, Path]:
        """Write one Cargo.lock and matching-shape SPDX JSON fixture."""
        cargo_lock = directory / "Cargo.lock"
        cargo_lock.write_text(
            """version = 4

[[package]]
name = "psychometrics-commons-runtime"
version = "0.1.0"

[[package]]
name = "serde"
version = "1.0.219"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
""",
            encoding="utf-8",
        )
        sbom = directory / "sbom.spdx.json"
        sbom.write_text(
            json.dumps(
                {
                    "spdxVersion": spdx_version,
                    "dataLicense": "CC0-1.0",
                    "SPDXID": "SPDXRef-DOCUMENT",
                    "packages": packages,
                }
            ),
            encoding="utf-8",
        )
        return sbom, cargo_lock

    def test_complete_locked_dependency_passes(self) -> None:
        """A generated package with the locked name/version satisfies the evidence gate."""
        with tempfile.TemporaryDirectory() as temporary_directory:
            sbom, cargo_lock = self.write_fixture(
                Path(temporary_directory),
                packages=[{"name": "serde", "versionInfo": "1.0.219"}],
            )
            validate_sbom(sbom, cargo_lock)

    def test_missing_locked_dependency_fails_closed(self) -> None:
        """An SBOM cannot be retained when a locked third-party package disappeared."""
        with tempfile.TemporaryDirectory() as temporary_directory:
            sbom, cargo_lock = self.write_fixture(
                Path(temporary_directory),
                packages=[{"name": "other", "versionInfo": "9.9.9"}],
            )
            with self.assertRaisesRegex(SbomValidationError, "serde@1.0.219"):
                validate_sbom(sbom, cargo_lock)

    def test_non_spdx_two_document_fails_closed(self) -> None:
        """An unrelated JSON inventory must not be mislabeled as accepted SPDX evidence."""
        with tempfile.TemporaryDirectory() as temporary_directory:
            sbom, cargo_lock = self.write_fixture(
                Path(temporary_directory),
                packages=[{"name": "serde", "versionInfo": "1.0.219"}],
                spdx_version="SPDX-3.0",
            )
            with self.assertRaisesRegex(SbomValidationError, "supported SPDX 2.x"):
                validate_sbom(sbom, cargo_lock)

    def test_malformed_spdx_two_versions_fail_closed(self) -> None:
        """Incomplete or malformed SPDX 2.x-looking versions must not enter retained evidence."""
        for spdx_version in ("SPDX-2.", "SPDX-2.invalid"):
            with self.subTest(spdx_version=spdx_version):
                with tempfile.TemporaryDirectory() as temporary_directory:
                    sbom, cargo_lock = self.write_fixture(
                        Path(temporary_directory),
                        packages=[{"name": "serde", "versionInfo": "1.0.219"}],
                        spdx_version=spdx_version,
                    )
                    with self.assertRaisesRegex(SbomValidationError, "supported SPDX 2.x"):
                        validate_sbom(sbom, cargo_lock)

    def test_missing_cc0_data_license_fails_closed(self) -> None:
        """The required SPDX document data license is part of retained evidence validity."""
        with tempfile.TemporaryDirectory() as temporary_directory:
            sbom, cargo_lock = self.write_fixture(
                Path(temporary_directory),
                packages=[{"name": "serde", "versionInfo": "1.0.219"}],
            )
            payload = json.loads(sbom.read_text(encoding="utf-8"))
            payload["dataLicense"] = "NOASSERTION"
            sbom.write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaisesRegex(SbomValidationError, "CC0-1.0"):
                validate_sbom(sbom, cargo_lock)


if __name__ == "__main__":
    unittest.main()
