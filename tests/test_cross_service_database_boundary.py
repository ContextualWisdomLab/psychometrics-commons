"""Repository fitness tests for the cross-service database boundary.

Psychometrics Commons owns its product PostgreSQL store. Other ContextualWisdomLab
bounded contexts are consumed through versioned APIs, events, or artifacts, never
through their application databases. These tests make that architectural rule
machine-checkable without forbidding this service's own TEST_DATABASE_URL.
"""

from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# Only shipped/runtime/configuration surfaces are inspected. Tests deliberately live
# outside this set so the forbidden examples below do not match themselves.
RUNTIME_ROOTS = (
    ROOT / "src",
    ROOT / "migrations",
    ROOT / ".github" / "workflows",
)
RUNTIME_FILES = (
    ROOT / "Cargo.toml",
    ROOT / "rust-toolchain.toml",
)

EXTERNAL_CONTEXT_ENV_PREFIXES = (
    "FAST_MLSIRM",
    "KEYVERSE",
    "GYEOT",
    "TEPP",
    "SEMANTIC_DATA_PORTAL",
    "CONTEXTUAL_ORCHESTRATOR",
    "PG_LLM_BATCH",
    "EGRESSWEAVE",
    "INKSPAN",
    "RANKWEAVE",
    "LIFEOS",
    "CLEARFOLIO",
)

# A dedicated database URL/DSN for another bounded context is a high-signal sign
# that this service has crossed an ownership boundary. Product-owned generic
# TEST_DATABASE_URL remains intentionally allowed.
FORBIDDEN_EXTERNAL_DATABASE_TOKENS = tuple(
    f"{prefix}_{suffix}"
    for prefix in EXTERNAL_CONTEXT_ENV_PREFIXES
    for suffix in ("DATABASE_URL", "DB_URL", "DATABASE_DSN", "DB_DSN")
)

# These PostgreSQL facilities create direct cross-database/server coupling and are
# incompatible with the API/event/artifact-only dependency direction in ADR-0015.
FORBIDDEN_CROSS_DATABASE_SQL = re.compile(
    r"\b(?:postgres_fdw|dblink(?:_connect)?|CREATE\s+SERVER|IMPORT\s+FOREIGN\s+SCHEMA)\b",
    re.IGNORECASE,
)


def runtime_files() -> list[Path]:
    """Return deterministic UTF-8 text files that can affect shipped runtime behavior."""

    files: list[Path] = []
    for root in RUNTIME_ROOTS:
        if not root.exists():
            continue
        files.extend(path for path in root.rglob("*") if path.is_file())
    files.extend(path for path in RUNTIME_FILES if path.exists())
    return sorted(set(files))


def read_text(path: Path) -> str:
    """Read one repository-controlled runtime/configuration file as UTF-8 text."""

    return path.read_text(encoding="utf-8")


class CrossServiceDatabaseBoundaryTest(unittest.TestCase):
    """Keep read-only bounded-context dependencies out of this product database."""

    def test_external_context_database_credentials_are_not_runtime_inputs(self) -> None:
        violations: list[str] = []
        for path in runtime_files():
            text = read_text(path)
            for token in FORBIDDEN_EXTERNAL_DATABASE_TOKENS:
                if token in text:
                    violations.append(f"{path.relative_to(ROOT)}: {token}")

        self.assertEqual(
            [],
            violations,
            "cross-service database credentials are forbidden; use a versioned API, "
            "event, or artifact contract instead:\n" + "\n".join(violations),
        )

    def test_cross_database_sql_primitives_are_not_shipped(self) -> None:
        violations: list[str] = []
        for path in runtime_files():
            match = FORBIDDEN_CROSS_DATABASE_SQL.search(read_text(path))
            if match is not None:
                violations.append(
                    f"{path.relative_to(ROOT)}: {match.group(0)}"
                )

        self.assertEqual(
            [],
            violations,
            "direct PostgreSQL cross-database/server coupling is forbidden; use a "
            "versioned API, event, or artifact contract instead:\n"
            + "\n".join(violations),
        )


if __name__ == "__main__":
    unittest.main()
