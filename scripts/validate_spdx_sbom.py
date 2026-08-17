#!/usr/bin/env python3
"""Validate generated SPDX JSON against the repository's locked Rust dependencies.

The check is intentionally small and deterministic. It does not try to replace an SPDX schema
validator or a vulnerability scanner. It proves that the generated file is structurally recognizable
as a supported SPDX 2.x JSON document and that every registry/git dependency pinned by
``Cargo.lock`` is represented by name and version before the SBOM is retained as build evidence.
"""

from __future__ import annotations

import json
from pathlib import Path
import sys
import tomllib
from typing import Any


SUPPORTED_SPDX_VERSIONS = ("SPDX-2.0", "SPDX-2.1", "SPDX-2.2", "SPDX-2.3")


class SbomValidationError(ValueError):
    """Raised when generated SBOM evidence is missing or inconsistent with Cargo.lock."""


def _load_json(path: Path) -> dict[str, Any]:
    """Load one JSON object, rejecting non-object roots with an operator-readable error."""
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SbomValidationError(f"cannot read SPDX JSON from {path}: {error}") from error
    if not isinstance(payload, dict):
        raise SbomValidationError("SPDX JSON root must be an object")
    return payload


def _locked_external_packages(path: Path) -> set[tuple[str, str]]:
    """Return third-party name/version identities pinned by Cargo.lock."""
    try:
        cargo_lock = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise SbomValidationError(f"cannot read Cargo.lock from {path}: {error}") from error

    packages = cargo_lock.get("package")
    if not isinstance(packages, list):
        raise SbomValidationError("Cargo.lock must contain a package array")

    locked: set[tuple[str, str]] = set()
    for package in packages:
        if not isinstance(package, dict) or "source" not in package:
            continue
        name = package.get("name")
        version = package.get("version")
        if not isinstance(name, str) or not name or not isinstance(version, str) or not version:
            raise SbomValidationError("Cargo.lock external package is missing name or version")
        locked.add((name, version))
    if not locked:
        raise SbomValidationError("Cargo.lock contains no external packages to verify")
    return locked


def _spdx_packages(document: dict[str, Any]) -> set[tuple[str, str]]:
    """Return name/version identities declared by a supported SPDX 2.x JSON document."""
    spdx_version = document.get("spdxVersion")
    if spdx_version not in SUPPORTED_SPDX_VERSIONS:
        raise SbomValidationError("SBOM must declare a supported SPDX 2.x version (2.0 through 2.3)")
    if document.get("dataLicense") != "CC0-1.0":
        raise SbomValidationError("SPDX dataLicense must be CC0-1.0")

    packages = document.get("packages")
    if not isinstance(packages, list) or not packages:
        raise SbomValidationError("SPDX SBOM must contain at least one package")

    declared: set[tuple[str, str]] = set()
    for package in packages:
        if not isinstance(package, dict):
            continue
        name = package.get("name")
        version = package.get("versionInfo")
        if isinstance(name, str) and name and isinstance(version, str) and version:
            declared.add((name, version))
    if not declared:
        raise SbomValidationError("SPDX SBOM contains no package name/version identities")
    return declared


def validate_sbom(sbom_path: Path, cargo_lock_path: Path) -> None:
    """Fail when locked external Rust dependencies are absent from generated SPDX evidence."""
    document = _load_json(sbom_path)
    locked = _locked_external_packages(cargo_lock_path)
    declared = _spdx_packages(document)
    missing = sorted(locked - declared)
    if missing:
        preview = ", ".join(f"{name}@{version}" for name, version in missing[:10])
        suffix = "" if len(missing) <= 10 else f" (+{len(missing) - 10} more)"
        raise SbomValidationError(
            f"SPDX SBOM is missing {len(missing)} Cargo.lock package(s): {preview}{suffix}"
        )


def main(argv: list[str]) -> int:
    """Run the command-line validator and return a conventional process status."""
    if len(argv) != 3:
        print("usage: validate_spdx_sbom.py <sbom.spdx.json> <Cargo.lock>", file=sys.stderr)
        return 2
    try:
        validate_sbom(Path(argv[1]), Path(argv[2]))
    except SbomValidationError as error:
        print(f"SBOM validation failed: {error}", file=sys.stderr)
        return 1
    print("SPDX SBOM covers every external dependency pinned by Cargo.lock")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
