"""Guard the generated PostgreSQL numeric-codepoint ranges against Rust drift."""

from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
GENERATOR = ROOT / "scripts" / "generate_integration_numeric_ranges.rs"
MIGRATION = ROOT / "migrations" / "0001_integration_delivery.sql"


class IntegrationNumericRangeParityTest(unittest.TestCase):
    """Keep the SQL numeric set identical to the pinned Rust toolchain."""

    def test_migration_numeric_multirange_matches_rust_char_is_numeric(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            binary = Path(temporary_directory) / "generate-integration-numeric-ranges"
            subprocess.run(
                ["rustc", "--edition=2024", str(GENERATOR), "-o", str(binary)],
                cwd=ROOT,
                check=True,
            )
            generated = subprocess.check_output([str(binary)], cwd=ROOT, text=True).strip()

        migration = MIGRATION.read_text(encoding="utf-8")
        self.assertIn(
            f"'{generated}'::int4multirange",
            migration,
            "Regenerate the integration numeric multirange after changing the pinned Rust toolchain.",
        )


if __name__ == "__main__":
    unittest.main()
