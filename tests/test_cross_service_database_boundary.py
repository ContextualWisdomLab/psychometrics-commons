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
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]

# Only shipped/runtime/configuration surfaces are inspected. Tests deliberately live
# outside this set so the forbidden examples below do not match themselves.
RUNTIME_ROOTS = (
    ROOT / "src",
    ROOT / "migrations",
    ROOT / ".github" / "workflows",
    ROOT / ".github" / "actions",
)
RUNTIME_FILES = (
    ROOT / "Cargo.toml",
    ROOT / "rust-toolchain.toml",
    ROOT / "build.rs",
    ROOT / ".cargo" / "config",
    ROOT / ".cargo" / "config.toml",
)
RUNTIME_TEXT_SUFFIXES = frozenset({".rs", ".sql", ".toml", ".yml", ".yaml"})
DEPLOYMENT_DIRECTORY_NAMES = frozenset(
    {"deploy", "deployment", "deployments", "infra", "ops", "k8s", "helm"}
)
DEPLOYMENT_TEXT_SUFFIXES = frozenset({".json", ".toml", ".yml", ".yaml"})

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

# Dedicated database connection material for another bounded context is a high-signal
# sign that this service has crossed an ownership boundary. Product-owned generic
# TEST_DATABASE_URL remains intentionally allowed.
EXTERNAL_DATABASE_TOKEN_SUFFIXES = (
    "DATABASE_URL",
    "DB_URL",
    "DATABASE_DSN",
    "DB_DSN",
    "DATABASE_HOST",
    "DB_HOST",
    "DATABASE_PORT",
    "DB_PORT",
    "DATABASE_USER",
    "DB_USER",
    "DATABASE_USERNAME",
    "DB_USERNAME",
    "DATABASE_PASSWORD",
    "DB_PASSWORD",
    "DATABASE_NAME",
    "DB_NAME",
)
FORBIDDEN_EXTERNAL_DATABASE_TOKENS = tuple(
    f"{prefix}_{suffix}"
    for prefix in EXTERNAL_CONTEXT_ENV_PREFIXES
    for suffix in EXTERNAL_DATABASE_TOKEN_SUFFIXES
)

# These PostgreSQL facilities create direct cross-database/server coupling and are
# incompatible with the API/event/artifact-only dependency direction in ADR-0015.
FORBIDDEN_CROSS_DATABASE_SQL = re.compile(
    r"\b(?:postgres_fdw|dblink(?:_[a-z0-9_]+)?|CREATE\s+SERVER|IMPORT\s+FOREIGN\s+SCHEMA)\b",
    re.IGNORECASE,
)


def is_runtime_text_file(path: Path) -> bool:
    """Return whether a runtime path has a repository-scanned text suffix."""

    return path.suffix.lower() in RUNTIME_TEXT_SUFFIXES


def is_deployment_manifest(path: Path) -> bool:
    """Return whether a repository-relative path can configure a deployed runtime."""

    try:
        repository_path = path.relative_to(ROOT)
    except ValueError:
        return False

    name = repository_path.name.lower()
    if name == "dockerfile" or name.startswith("dockerfile."):
        return True
    if name in {"compose.yml", "compose.yaml", "docker-compose.yml", "docker-compose.yaml"}:
        return True
    if name == ".env" or name.startswith(".env."):
        return True
    return (
        any(
            part.lower() in DEPLOYMENT_DIRECTORY_NAMES
            for part in repository_path.parts
        )
        and repository_path.suffix.lower() in DEPLOYMENT_TEXT_SUFFIXES
    )


def runtime_files() -> list[Path]:
    """Return deterministic UTF-8 text files that can affect shipped runtime behavior."""

    files: list[Path] = []
    for root in RUNTIME_ROOTS:
        if not root.exists():
            continue
        files.extend(
            path
            for path in root.rglob("*")
            if path.is_file() and is_runtime_text_file(path)
        )
    files.extend(path for path in RUNTIME_FILES if path.exists())
    files.extend(
        path
        for path in ROOT.rglob("*")
        if path.is_file() and is_deployment_manifest(path)
    )
    return sorted(set(files))


def read_text(path: Path) -> str:
    """Read one expected repository-controlled UTF-8 runtime/configuration file."""

    return path.read_text(encoding="utf-8")


class CrossServiceDatabaseBoundaryTest(unittest.TestCase):
    """Keep read-only bounded-context dependencies out of this product database."""

    def test_runtime_scan_accepts_only_expected_text_artifacts(self) -> None:
        """Classify runtime text by suffix without relying on fixture existence."""

        self.assertTrue(is_runtime_text_file(ROOT / "src" / "lib.rs"))
        self.assertTrue(
            is_runtime_text_file(ROOT / ".github" / "workflows" / "ci.yml")
        )
        self.assertTrue(is_runtime_text_file(ROOT / "src" / "future_module.rs"))
        self.assertFalse(is_runtime_text_file(ROOT / "src" / "fixture.png"))
        self.assertFalse(is_runtime_text_file(ROOT / "migrations" / "fixture.bin"))

    def test_deployment_manifest_shapes_are_runtime_inputs(self) -> None:
        """Treat common deploy-time manifests as architecture-enforcement inputs."""

        for path in (
            ROOT / "Dockerfile",
            ROOT / "Dockerfile.production",
            ROOT / "compose.yaml",
            ROOT / "docker-compose.yml",
            ROOT / "deploy" / "service.yaml",
            ROOT / "infra" / "runtime.json",
            ROOT / "k8s" / "values.toml",
            ROOT / ".env.production",
        ):
            self.assertTrue(is_deployment_manifest(path), str(path))
        self.assertFalse(is_deployment_manifest(ROOT / "docs" / "example.json"))

    def test_build_and_composite_action_inputs_are_in_scan_contract(self) -> None:
        """Do not let build hooks or reusable Actions bypass the database boundary gate."""

        self.assertIn(ROOT / "build.rs", RUNTIME_FILES)
        self.assertIn(ROOT / ".cargo" / "config", RUNTIME_FILES)
        self.assertIn(ROOT / ".cargo" / "config.toml", RUNTIME_FILES)
        self.assertIn(ROOT / ".github" / "actions", RUNTIME_ROOTS)

    def test_deployment_manifest_scope_is_repository_relative(self) -> None:
        """Ignore deployment-like checkout ancestors and paths outside the repository."""

        fake_root = Path("/tmp/ops/psychometrics-commons")
        with patch(f"{__name__}.ROOT", fake_root):
            self.assertFalse(is_deployment_manifest(fake_root / "docs" / "example.json"))
            self.assertTrue(is_deployment_manifest(fake_root / "deploy" / "service.yaml"))
            self.assertFalse(is_deployment_manifest(Path("/tmp/deploy/external.yaml")))

    def test_every_dblink_function_family_is_forbidden(self) -> None:
        """Reject representative members of PostgreSQL's remote dblink family."""

        for sql in (
            "SELECT dblink('dbname=other', 'SELECT 1')",
            "SELECT dblink_exec('remote', 'DELETE FROM sample')",
            "SELECT dblink_connect_u('remote', 'dbname=other')",
            "SELECT dblink_send_query('remote', 'SELECT 1')",
            "SELECT dblink_build_sql_insert('sample', ARRAY[1], 1, ARRAY['1'], ARRAY['2'])",
        ):
            self.assertIsNotNone(FORBIDDEN_CROSS_DATABASE_SQL.search(sql), sql)

    def test_sibling_database_connection_parts_are_forbidden(self) -> None:
        """Cover split host, port, identity, secret, and database-name configuration."""

        for token in (
            "KEYVERSE_DB_HOST",
            "TEPP_DATABASE_PORT",
            "GYEOT_DB_USER",
            "FAST_MLSIRM_DATABASE_USERNAME",
            "INKSPAN_DB_PASSWORD",
            "RANKWEAVE_DATABASE_NAME",
            "CLEARFOLIO_DATABASE_URL",
            "LIFEOS_DB_DSN",
        ):
            self.assertIn(token, FORBIDDEN_EXTERNAL_DATABASE_TOKENS)

        self.assertNotIn("TEST_DATABASE_URL", FORBIDDEN_EXTERNAL_DATABASE_TOKENS)
        self.assertNotIn("KEYVERSE_API_URL", FORBIDDEN_EXTERNAL_DATABASE_TOKENS)

    def test_external_context_database_credentials_are_not_runtime_inputs(self) -> None:
        """Reject dedicated database connection material for read-only sibling contexts."""

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
        """Reject PostgreSQL primitives that would bypass service-owned contracts."""

        violations: list[str] = []
        for path in runtime_files():
            match = FORBIDDEN_CROSS_DATABASE_SQL.search(read_text(path))
            if match is not None:
                violations.append(f"{path.relative_to(ROOT)}: {match.group(0)}")

        self.assertEqual(
            [],
            violations,
            "direct PostgreSQL cross-database/server coupling is forbidden; use a "
            "versioned API, event, or artifact contract instead:\n"
            + "\n".join(violations),
        )


if __name__ == "__main__":
    unittest.main()
