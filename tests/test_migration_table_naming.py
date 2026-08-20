"""Contract tests for descriptive PostgreSQL table names in owned migrations."""

from __future__ import annotations

import re
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
MIGRATIONS_DIR = REPOSITORY_ROOT / "migrations"
CREATE_TABLE_PATTERN = re.compile(
    r"^\s*CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?"
    r"(?P<qualified_name>(?:\"[^\"]+\"|[A-Za-z_][A-Za-z0-9_]*)"
    r"(?:\.(?:\"[^\"]+\"|[A-Za-z_][A-Za-z0-9_]*))?)\s*\(",
    re.IGNORECASE | re.MULTILINE,
)
DESCRIPTIVE_SNAKE_CASE_PATTERN = re.compile(
    r"^[a-z][a-z0-9]*(?:_[a-z0-9]+)+$"
)


def created_table_names(sql: str) -> list[str]:
    """Return table identifiers declared by CREATE TABLE statements."""
    return [
        match.group("qualified_name").rsplit(".", maxsplit=1)[-1]
        for match in CREATE_TABLE_PATTERN.finditer(sql)
    ]


def is_descriptive_snake_case_table_name(table_name: str) -> bool:
    """Require an unquoted lower snake_case table name with at least two words."""
    return DESCRIPTIVE_SNAKE_CASE_PATTERN.fullmatch(table_name) is not None


class MigrationTableNamingContractTests(unittest.TestCase):
    """Keep product-owned PostgreSQL tables descriptive and machine-reviewable."""

    def test_parser_covers_plain_if_not_exists_and_schema_qualified_tables(self) -> None:
        sql = """
        CREATE TABLE assessment_session (session_ref text);
        CREATE TABLE IF NOT EXISTS public.scoring_job_attempt (attempt_ref text);
        """

        self.assertEqual(
            created_table_names(sql),
            ["assessment_session", "scoring_job_attempt"],
        )

    def test_name_contract_rejects_single_word_mixed_case_and_quoted_aliases(self) -> None:
        for invalid_name in ["session", "AssessmentSession", '"assessment_session"']:
            with self.subTest(invalid_name=invalid_name):
                self.assertFalse(is_descriptive_snake_case_table_name(invalid_name))

        for valid_name in ["assessment_session", "scoring_job_attempt", "result_snapshot_v2"]:
            with self.subTest(valid_name=valid_name):
                self.assertTrue(is_descriptive_snake_case_table_name(valid_name))

    def test_every_owned_migration_table_uses_descriptive_snake_case(self) -> None:
        observed: list[tuple[Path, str]] = []
        invalid: list[str] = []

        for migration_path in sorted(MIGRATIONS_DIR.glob("*.sql")):
            sql = migration_path.read_text(encoding="utf-8")
            for table_name in created_table_names(sql):
                observed.append((migration_path, table_name))
                if not is_descriptive_snake_case_table_name(table_name):
                    invalid.append(f"{migration_path.relative_to(REPOSITORY_ROOT)}:{table_name}")

        self.assertTrue(observed, "migration naming gate found no CREATE TABLE statements")
        self.assertEqual(
            invalid,
            [],
            "owned PostgreSQL table names must be unquoted descriptive two-or-more-word "
            f"snake_case identifiers; invalid={invalid}",
        )


if __name__ == "__main__":
    unittest.main()
