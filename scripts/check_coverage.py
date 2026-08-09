"""Fail closed unless an LLVM coverage report is exactly complete."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Mapping, Sequence


def load_totals(path: Path) -> Mapping[str, Any]:
    """Load the single-report totals mapping from LLVM coverage JSON."""
    payload = json.loads(path.read_text(encoding="utf-8"))
    data = payload.get("data")
    if not isinstance(data, list) or len(data) != 1:
        raise ValueError("coverage JSON must contain exactly one data entry")
    totals = data[0].get("totals")
    if not isinstance(totals, Mapping):
        raise ValueError("coverage JSON data entry must contain totals")
    return totals


def validate_kind(totals: Mapping[str, Any], kind: str) -> str:
    """Return a stable success message or raise for incomplete coverage."""
    summary = totals.get(kind)
    if not isinstance(summary, Mapping):
        raise ValueError(f"coverage totals do not contain {kind}")
    count = summary.get("count")
    covered = summary.get("covered")
    if type(count) is not int or type(covered) is not int:
        raise ValueError(f"{kind} count and covered values must be integers")
    if count < 0 or covered < 0 or covered > count:
        raise ValueError(f"{kind} coverage counts are invalid")
    if covered != count:
        raise ValueError(f"{kind} coverage is incomplete: {covered}/{count}")
    return f"{kind} coverage: PASS ({covered}/{count}, 100%)"


def validate_report(path: Path, kinds: Sequence[str]) -> list[str]:
    """Validate all requested coverage kinds in a report."""
    totals = load_totals(path)
    return [validate_kind(totals, kind) for kind in kinds]


def build_parser() -> argparse.ArgumentParser:
    """Create the command-line argument parser."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument(
        "--kind",
        action="append",
        choices=("lines", "branches"),
        required=True,
        dest="kinds",
    )
    return parser


def main() -> int:
    """Validate one LLVM coverage report from command-line arguments."""
    namespace = build_parser().parse_args()
    try:
        messages = validate_report(namespace.report, namespace.kinds)
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"Coverage contract: FAIL: {error}", file=sys.stderr)
        return 1
    for message in messages:
        print(message)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
