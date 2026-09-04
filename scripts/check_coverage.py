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


def validate_lcov_kind(path: Path, kind: str) -> str:
    """Validate merged LCOV records without counting duplicate instantiations."""
    if kind not in {"lines", "branches"}:
        raise ValueError(f"unsupported LCOV coverage kind: {kind}")
    record_prefix = "DA:" if kind == "lines" else "BRDA:"
    count = 0
    covered = 0
    branch_coverage: dict[tuple[str, int, str, str], bool] = {}
    source: str | None = None
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if line.startswith("SF:"):
            if source is not None:
                raise ValueError("LCOV source records must end before another source starts")
            source = line[3:]
            if not source:
                raise ValueError("LCOV source records must name a file")
        elif line == "end_of_record":
            if source is None:
                raise ValueError("LCOV end records must follow a source record")
            source = None
        elif line.startswith(record_prefix):
            if source is None:
                raise ValueError(f"LCOV {kind} record is outside a source record")
            fields = line[len(record_prefix) :].split(",")
            if kind == "lines":
                if len(fields) < 2:
                    raise ValueError("LCOV line records must contain a line and hit count")
                try:
                    line_number = int(fields[0])
                    hits = int(fields[1])
                except ValueError as error:
                    raise ValueError("LCOV line records must contain integer values") from error
                if line_number < 0 or hits < 0:
                    raise ValueError("LCOV line numbers and hit counts cannot be negative")
            else:
                if len(fields) != 4:
                    raise ValueError("LCOV branch records must contain four fields")
                try:
                    line_number = int(fields[0])
                except ValueError as error:
                    raise ValueError("LCOV branch records must contain an integer line") from error
                if line_number < 0:
                    raise ValueError("LCOV branch line numbers cannot be negative")
                if fields[3] == "-":
                    hits = 0
                else:
                    try:
                        hits = int(fields[3])
                    except ValueError as error:
                        raise ValueError(
                            "LCOV branch records must contain an integer hit count"
                        ) from error
                    if hits < 0:
                        raise ValueError("LCOV branch hit counts cannot be negative")
                key = (source, line_number, fields[1], fields[2])
                branch_coverage[key] = branch_coverage.get(key, False) or hits > 0
                continue
            count += 1
            covered += int(hits > 0)
    if kind == "branches":
        count = len(branch_coverage)
        covered = sum(branch_coverage.values())
    if source is not None:
        raise ValueError("LCOV report ended before the source record was closed")
    if count == 0:
        raise ValueError(f"LCOV report does not contain {kind} records")
    if covered != count:
        raise ValueError(f"{kind} coverage is incomplete: {covered}/{count}")
    return f"{kind} coverage: PASS ({covered}/{count}, 100%)"


def validate_report(path: Path, kinds: Sequence[str]) -> list[str]:
    """Validate all requested coverage kinds in a report."""
    if path.suffix == ".lcov":
        return [validate_lcov_kind(path, kind) for kind in kinds]
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
