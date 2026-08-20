"""Contracts for descriptive, collision-resistant PostgreSQL migration names."""

from __future__ import annotations

import re
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
MIGRATIONS_DIR = REPOSITORY_ROOT / "migrations"
CREATE_TABLE_PATTERN = re.compile(
    r"^\s*CREATE\s+(?:UNLOGGED\s+)?TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?"
    r"(?P<qualified_name>(?:\"[^\"]+\"|[A-Za-z_][A-Za-z0-9_]*)"
    r"(?:\.(?:\"[^\"]+\"|[A-Za-z_][A-Za-z0-9_]*))?)\s*\(",
    re.IGNORECASE | re.MULTILINE,
)
DESCRIPTIVE_SNAKE_CASE_PATTERN = re.compile(
    r"^[a-z][a-z0-9]*(?:_[a-z0-9]+)+$"
)
MIGRATION_FILENAME_PATTERN = re.compile(
    r"^(?P<number>[0-9]{4})_(?P<slug>[a-z][a-z0-9]*(?:_[a-z0-9]+)+)\.sql$"
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
    """Keep product-owned PostgreSQL migration identity machine-reviewable."""

    def test_parser_covers_postgresql_table_creation_forms(self) -> None:
        sql = """
        CREATE TABLE assessment_session (session_ref text);
        CREATE TABLE IF NOT EXISTS public.scoring_job_attempt (attempt_ref text);
        CREATE UNLOGGED TABLE durable_import_buffer (row_ref text);
        CREATE GLOBAL TEMPORARY TABLE migration_review_cache (row_ref text);
        CREATE TABLE typed_result_snapshot OF result_record;
        CREATE TABLE monthly_result_partition PARTITION OF result_archive DEFAULT;
        CREATE UNLOGGED TABLE report_extract AS SELECT 1 AS value;
        """

        self.assertEqual(
            created_table_names(sql),
            [
                "assessment_session",
                "scoring_job_attempt",
                "durable_import_buffer",
                "migration_review_cache",
                "typed_result_snapshot",
                "monthly_result_partition",
                "report_extract",
            ],
        )

    def test_name_contract_rejects_single_word_mixed_case_and_quoted_aliases(self) -> None:
        for invalid_name in ["session", "AssessmentSession", '"assessment_session"']:
            with self.subTest(invalid_name=invalid_name):
                self.assertFalse(is_descriptive_snake_case_table_name(invalid_name))

        for valid_name in ["assessment_session", "scoring_job_attempt", "result_snapshot_v2"]:
            with self.subTest(valid_name=valid_name):
                self.assertTrue(is_descriptive_snake_case_table_name(valid_name))

    def test_migration_filenames_keep_unique_four_digit_descriptive_identity(self) -> None:
        migration_paths = sorted(MIGRATIONS_DIR.glob("*.sql"))
        self.assertTrue(migration_paths, "migration naming gate found no SQL migrations")

        seen_numbers: dict[str, Path] = {}
        invalid_filenames: list[str] = []
        duplicate_numbers: list[str] = []

        for migration_path in migration_paths:
            match = MIGRATION_FILENAME_PATTERN.fullmatch(migration_path.name)
            if match is None:
                invalid_filenames.append(migration_path.name)
                continue

            migration_number = match.group("number")
            previous = seen_numbers.get(migration_number)
            if previous is not None:
                duplicate_numbers.append(
                    f"{migration_number}:{previous.name},{migration_path.name}"
                )
            else:
                seen_numbers[migration_number] = migration_path

        self.assertEqual(
            invalid_filenames,
            [],
            "migration files must use NNNN_<two-or-more-word-snake-case>.sql; "
            f"invalid={invalid_filenames}",
        )
        self.assertEqual(
            duplicate_numbers,
            [],
            "migration sequence identities must be unique; "
            f"duplicates={duplicate_numbers}",
        )

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
