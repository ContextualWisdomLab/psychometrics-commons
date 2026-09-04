"""Executable contract for fail-closed source-distribution legal readiness."""

from pathlib import Path
import tempfile
import unittest


SCRIPT = Path("scripts/check_release_legal_readiness.py")
WORKFLOW = Path(".github/workflows/release-legal-readiness.yml")
RUNTIME_CI = Path(".github/workflows/ci.yml")
PINNED_SETUP_PYTHON = "actions/setup-python@ece7cb06caefa5fff74198d8649806c4678c61a1"


def _yaml_list_scalar(value: str) -> str:
    """Return a simple YAML list scalar without depending on quote style."""
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
        return value[1:-1]
    return value


def _workflow_event_paths(workflow_text: str, event_name: str) -> set[str]:
    """Read one block-style event's ``paths`` entries without binding to formatting order."""
    lines = workflow_text.splitlines()
    event_index = next(
        (index for index, line in enumerate(lines) if line.strip() == f"{event_name}:"),
        None,
    )
    if event_index is None:
        return set()

    event_indent = len(lines[event_index]) - len(lines[event_index].lstrip())
    paths_indent: int | None = None
    paths: set[str] = set()

    for line in lines[event_index + 1 :]:
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(line) - len(line.lstrip())
        if indent <= event_indent:
            break
        if paths_indent is None:
            if stripped == "paths:":
                paths_indent = indent
            continue
        if indent <= paths_indent:
            break
        if stripped.startswith("- "):
            paths.add(_yaml_list_scalar(stripped[2:]))

    return paths


class ReleaseLegalReadinessContract(unittest.TestCase):
    """Require explicit license evidence without inventing a license choice."""

    def test_repository_owns_a_deterministic_readiness_checker(self) -> None:
        """The preflight checker must exist before any release can claim legal readiness."""
        self.assertTrue(SCRIPT.is_file())
        source = SCRIPT.read_text(encoding="utf-8")
        self.assertIn("LICENSE_CANDIDATES", source)
        self.assertIn("license-file", source)
        self.assertIn("license", source)
        self.assertIn("ReleaseLegalReadinessError", source)

    def test_preflight_is_manual_fail_closed_and_read_only(self) -> None:
        """A manual operator gate must not publish, mutate, or silently waive missing evidence."""
        self.assertTrue(WORKFLOW.is_file())
        text = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("workflow_dispatch:", text)
        self.assertNotIn("pull_request:", text)
        self.assertNotIn("release:", text)
        self.assertIn("contents: read", text)
        self.assertNotIn("contents: write", text)
        self.assertNotIn("id-token: write", text)
        self.assertIn("persist-credentials: false", text)
        self.assertIn('test "$GITHUB_REF" = "refs/heads/main"', text)
        self.assertIn(PINNED_SETUP_PYTHON, text)
        self.assertIn("python-version: '3.13'", text)
        self.assertIn("python3 tests/test_release_legal_readiness.py", text)
        self.assertIn("python3 tests/test_check_release_legal_readiness.py", text)
        self.assertIn("python3 scripts/check_release_legal_readiness.py .", text)

    def test_runtime_ci_cannot_skip_changes_to_release_preflight_sources(self) -> None:
        """Changing the checker, workflow, or root license evidence must trigger the contract suite."""
        self.assertTrue(RUNTIME_CI.is_file())
        text = RUNTIME_CI.read_text(encoding="utf-8")
        required_paths = {
            "scripts/check_release_legal_readiness.py",
            ".github/workflows/release-legal-readiness.yml",
            "LICENSE*",
            "COPYING*",
        }
        for event_name in ("pull_request", "push"):
            event_paths = _workflow_event_paths(text, event_name)
            self.assertTrue(
                required_paths.issubset(event_paths),
                f"{event_name} paths must include every legal-readiness source: "
                f"missing {sorted(required_paths - event_paths)}",
            )
        self.assertIn(PINNED_SETUP_PYTHON, text)
        self.assertIn("python-version: '3.13'", text)
        self.assertIn("python3 -m unittest discover -s tests -p 'test_*.py' -v", text)

    def test_runtime_path_parser_ignores_quote_style_order_and_indentation_width(self) -> None:
        """Equivalent block-style workflow formatting must not invalidate the contract helper."""
        equivalent = """\
on:
    pull_request:
        paths:
            - COPYING*
            - 'LICENSE*'
            - .github/workflows/release-legal-readiness.yml
            - "scripts/check_release_legal_readiness.py"
    push:
        paths:
          - scripts/check_release_legal_readiness.py
          - "COPYING*"
          - '.github/workflows/release-legal-readiness.yml'
          - LICENSE*
"""
        expected = {
            "scripts/check_release_legal_readiness.py",
            ".github/workflows/release-legal-readiness.yml",
            "LICENSE*",
            "COPYING*",
        }
        self.assertEqual(_workflow_event_paths(equivalent, "pull_request"), expected)
        self.assertEqual(_workflow_event_paths(equivalent, "push"), expected)

    def test_fixture_directory_can_be_created_without_touching_repository(self) -> None:
        """Unit tests for the checker must use isolated fixtures rather than changing root rights evidence."""
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            (root / "Cargo.toml").write_text("[package]\nname = \"fixture\"\n", encoding="utf-8")
            self.assertTrue((root / "Cargo.toml").is_file())


if __name__ == "__main__":
    unittest.main()
