#!/usr/bin/env python3
"""Fail closed when source-distribution license evidence is incomplete.

This preflight intentionally does not choose a license, interpret legal terms, or claim that the
repository has sufficient rights to distribute any assessment content. It only checks that a human-
reviewed repository license is discoverable and that Cargo package metadata points at explicit
license terms before an operator treats a source package as release-ready.
"""

from __future__ import annotations

import json
from pathlib import Path
import sys
import tomllib
from typing import Any


LICENSE_CANDIDATES = (
    "LICENSE",
    "LICENSE.md",
    "LICENSE.txt",
    "COPYING",
    "COPYING.md",
    "COPYING.txt",
)


class ReleaseLegalReadinessError(ValueError):
    """Raised when repository legal-readiness inputs cannot be inspected safely."""


def _read_manifest(path: Path) -> dict[str, Any]:
    """Load Cargo.toml as a table or fail with an operator-readable error."""
    try:
        manifest = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ReleaseLegalReadinessError(f"cannot read {path}: {error}") from error
    if not isinstance(manifest, dict):
        raise ReleaseLegalReadinessError("Cargo.toml root must be a table")
    package = manifest.get("package")
    if not isinstance(package, dict):
        raise ReleaseLegalReadinessError("Cargo.toml must contain a [package] table")
    return package


def _nonempty_string(value: object) -> str | None:
    """Return a stripped non-empty string, otherwise None."""
    if not isinstance(value, str):
        return None
    stripped = value.strip()
    return stripped or None


def _resolved_repo_file(root: Path, relative_path: str) -> Path | None:
    """Return non-empty terms evidence only when the resolved regular file stays in the repository."""
    root_resolved = root.resolve()
    candidate = (root / relative_path).resolve()
    try:
        candidate.relative_to(root_resolved)
    except ValueError:
        return None
    if not candidate.is_file():
        return None
    try:
        has_terms = bool(candidate.read_bytes().strip())
    except OSError:
        return None
    return candidate if has_terms else None


def evaluate_repository(root: Path) -> dict[str, object]:
    """Return deterministic evidence and blockers without interpreting license sufficiency."""
    root = root.resolve()
    if not root.is_dir():
        raise ReleaseLegalReadinessError(f"repository root is not a directory: {root}")

    package = _read_manifest(root / "Cargo.toml")
    standard_license_files = [
        candidate
        for candidate in LICENSE_CANDIDATES
        if _resolved_repo_file(root, candidate) is not None
    ]
    license_expression = _nonempty_string(package.get("license"))
    license_file_value = _nonempty_string(package.get("license-file"))
    declared_license_file = (
        _resolved_repo_file(root, license_file_value) if license_file_value is not None else None
    )

    blockers: list[str] = []
    if not standard_license_files and declared_license_file is None:
        blockers.append(
            "repository has no discoverable license file; an authorized owner must add reviewed license terms"
        )
    if license_expression is None and license_file_value is None:
        blockers.append(
            "Cargo.toml [package] has neither license nor license-file metadata"
        )
    if license_file_value is not None and declared_license_file is None:
        blockers.append(
            "Cargo.toml license-file is missing, empty, not a regular file, unreadable, or escapes the repository root"
        )

    evidence: dict[str, object] = {
        "ready": not blockers,
        "standard_license_files": sorted(standard_license_files),
        "cargo_license_expression_declared": license_expression is not None,
        "cargo_license_file": license_file_value,
        "cargo_license_file_resolved": (
            str(declared_license_file.relative_to(root)) if declared_license_file is not None else None
        ),
        "blockers": blockers,
        "limitations": [
            "presence checks do not establish copyright ownership, instrument rights, compatibility, or legal sufficiency",
            "this checker never selects or interprets license terms",
        ],
    }
    return evidence


def main(argv: list[str]) -> int:
    """Print machine-readable evidence and return nonzero while release blockers remain."""
    if len(argv) != 2:
        print("usage: check_release_legal_readiness.py <repository-root>", file=sys.stderr)
        return 2
    try:
        evidence = evaluate_repository(Path(argv[1]))
    except ReleaseLegalReadinessError as error:
        print(json.dumps({"ready": False, "error": str(error)}, sort_keys=True), file=sys.stderr)
        return 2
    print(json.dumps(evidence, indent=2, sort_keys=True))
    return 0 if evidence["ready"] else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
