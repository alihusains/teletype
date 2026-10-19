#!/usr/bin/env python3
"""Test-inventory freeze: every test suite declares the safety claim it protects.

A committed baseline (``docs/test-inventory.json``) maps every discovered test
suite to exactly one claim id from a fixed taxonomy. The check fails when a
NEW suite appears without declaring its claim in a header comment, and when
the baseline goes stale (deleted files, duplicate paths, claims dropped from
the taxonomy). The freeze is enforcement of declared intent, not a count
target.

Discovery (exact):
  - Rust suites: every ``*.rs`` under ``crates/*/tests/`` at any depth,
    excluding any path containing a ``fixtures`` directory segment.
  - UI suites: every ``*.test.ts`` and ``*.test.tsx`` under ``ui/src/``.
  - Nothing else: no inline ``#[cfg(test)]`` modules, no ``scripts/e2e/*``,
    no ``evals/``, no ``ew/``.

Claim header rule (NEW suites only):
  A suite that is not in the baseline must declare its claim on one of its
  first 15 lines: a comment line whose text after ``//``, ``///`` or
  ``//!`` is exactly ``claim: <id>`` (case-sensitive id; whitespace around
  the parts is allowed, nothing else on the line). The id must be a taxonomy
  id from the baseline ``claims`` map; ``unclassified`` is baseline-only and
  rejected in headers. Baseline suites need no header edits.

Exit codes (like the other scripts/ checks):
  0  check passed
  1  inventory violation
  2  usage error (bad arguments, unreadable baseline, malformed JSON)

Usage:
    python3 scripts/test-inventory.py --check        (default)
    python3 scripts/test-inventory.py --update
    python3 scripts/test-inventory.py --check --baseline /path/to/copy.json
    python3 scripts/test-inventory.py --root /other/repo
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

SCHEMA = "teletype-test-inventory-v1"
MIN_SUITES = 30
HEADER_LINES = 15

# A comment line whose payload (after //, /// or //!) is exactly "claim: <id>".
# ``//``, ``///`` and ``//!`` all match: ``^//`` then an optional one of
# ``/`` or ``!``, then whitespace, then the payload, then end of line.
CLAIM_RE = re.compile(r"^\s*//[/!]?\s+claim:\s*([A-Za-z0-9-]+)\s*$")


def discover(root: Path) -> list[str]:
    """Return sorted repo-relative paths of all in-scope test suites."""
    paths: set[str] = set()
    for tests_dir in root.glob("crates/*/tests"):
        if not tests_dir.is_dir():
            continue
        for f in tests_dir.rglob("*.rs"):
            if not f.is_file():
                continue
            rel = f.relative_to(root)
            if "fixtures" in rel.parts:
                continue
            paths.add(rel.as_posix())
    ui_src = root / "ui" / "src"
    if ui_src.is_dir():
        for f in list(ui_src.rglob("*.test.ts")) + list(ui_src.rglob("*.test.tsx")):
            if f.is_file():
                paths.add(f.relative_to(root).as_posix())
    return sorted(paths)


def read_header(root: Path, rel: str) -> str | None:
    """Return the claim id from the file's first HEADER_LINES lines, or None."""
    try:
        text = (root / rel).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return None
    for line in text.splitlines()[:HEADER_LINES]:
        m = CLAIM_RE.match(line)
        if m:
            return m.group(1)
    return None


def load_baseline(path: Path) -> dict:
    """Load and validate the baseline JSON. Raises ValueError on problems."""
    try:
        with open(path, encoding="utf-8") as fh:
            data = json.load(fh)
    except OSError as e:
        raise ValueError(f"cannot read baseline {path}: {e}") from e
    except json.JSONDecodeError as e:
        raise ValueError(f"malformed JSON in baseline {path}: {e}") from e
    if not isinstance(data, dict) or data.get("schema") != SCHEMA:
        raise ValueError(f"{path}: missing or wrong \"schema\" (want {SCHEMA!r})")
    if not isinstance(data.get("claims"), dict) or not data["claims"]:
        raise ValueError(f"{path}: \"claims\" must be a non-empty object")
    suites = data.get("suites")
    if not isinstance(suites, list):
        raise ValueError(f"{path}: \"suites\" must be an array")
    for entry in suites:
        if not isinstance(entry, dict) or not isinstance(entry.get("path"), str) \
                or not isinstance(entry.get("claim"), str):
            raise ValueError(f"{path}: each suite entry needs string \"path\" and \"claim\"")
    return data


def check(root: Path, baseline: dict) -> int:
    taxonomy = baseline["claims"]
    suites = baseline["suites"]
    found = discover(root)
    failures: list[str] = []

    # Fail-closed guard: an empty or near-empty parse must never pass.
    if len(found) < MIN_SUITES:
        print(f"FAIL: parser found {len(found)} suites; expected >= {MIN_SUITES} — "
              f"refusing to pass on an empty parse")
        return 1

    # Duplicate suite paths in the baseline.
    seen: set[str] = set()
    for entry in suites:
        if entry["path"] in seen:
            failures.append(f"duplicate suite path in baseline: {entry['path']}")
        seen.add(entry["path"])
    baseline_paths = seen

    # New suites must carry a valid claim header.
    for rel in found:
        if rel in baseline_paths:
            continue
        header_claim = read_header(root, rel)
        if header_claim is None:
            failures.append(
                f"new suite has no claim header: {rel}\n"
                f"      add a header line `// claim: <id>` (or `///` / `//!`) "
                f"within the first {HEADER_LINES} lines.\n"
                f"      Valid ids: {', '.join(sorted(taxonomy))}"
            )
            continue
        if header_claim not in taxonomy:
            failures.append(
                f"new suite {rel} declares unknown claim id {header_claim!r}\n"
                f"      Valid ids: {', '.join(sorted(taxonomy))}"
            )
            continue
        if header_claim == "unclassified":
            failures.append(
                f"new suite {rel} declares `claim: unclassified` — "
                f"`unclassified` is baseline-only and must not be used in a header"
            )
            continue
        print(f"OK: new suite declares a claim (not in baseline yet): "
              f"{rel} -> {header_claim}")

    # Baseline entries must exist on disk.
    for rel in sorted(baseline_paths):
        if rel not in found:
            failures.append(
                f"stale baseline entry, file missing on disk: {rel}\n"
                f"      run `python3 scripts/test-inventory.py --update`"
            )

    # Baseline claims must be in the taxonomy.
    for entry in suites:
        if entry["claim"] not in taxonomy:
            failures.append(
                f"baseline claim id not in taxonomy: "
                f"{entry['path']} -> {entry['claim']!r}"
            )

    # A claim header that contradicts the baseline claim.
    for entry in suites:
        rel = entry["path"]
        if rel not in found:
            continue
        header_claim = read_header(root, rel)
        if header_claim is not None and header_claim != entry["claim"]:
            failures.append(
                f"claim header contradicts baseline: {rel} header says "
                f"{header_claim!r}, baseline says {entry['claim']!r}"
            )

    if failures:
        for f in failures:
            print(f"FAIL: {f}")
        return 1

    print(f"OK: test inventory in sync — {len(found)} suites, "
          f"baseline entries: {len(suites)}")
    return 0


def update(root: Path, baseline: dict, baseline_path: Path) -> int:
    taxonomy = baseline["claims"]
    found = discover(root)
    found_set = set(found)

    failures: list[str] = []
    kept: list[dict] = []
    dropped = 0
    added = 0
    skipped: list[str] = []

    for entry in baseline["suites"]:
        rel = entry["path"]
        if rel in found_set:
            kept.append({"path": rel, "claim": entry["claim"]})
        else:
            dropped += 1
            print(f"removed (file no longer exists): {rel}")

    baseline_paths = {e["path"] for e in baseline["suites"]}
    for rel in found:
        if rel in baseline_paths:
            continue
        header_claim = read_header(root, rel)
        if header_claim is None:
            skipped.append(rel)
            continue
        if header_claim not in taxonomy:
            skipped.append(rel)
            continue
        if header_claim == "unclassified":
            skipped.append(rel)
            continue
        kept.append({"path": rel, "claim": header_claim})
        added += 1
        print(f"added (claim header): {rel} -> {header_claim}")

    for rel in skipped:
        failures.append(
            f"new suite not added (no valid claim header): {rel}\n"
            f"      add a header line `// claim: <id>` (or `///` / `//!`) "
            f"within the first {HEADER_LINES} lines.\n"
            f"      Valid ids: {', '.join(sorted(taxonomy))}"
        )

    if failures:
        for f in failures:
            print(f"FAIL: {f}")
        print(f"updated: +{added} added, -{dropped} removed "
              f"(baseline NOT written while new suites are undeclared)")
        return 1

    kept.sort(key=lambda e: e["path"])
    out = {
        "schema": SCHEMA,
        "claims": taxonomy,
        "suites": kept,
    }
    try:
        with open(baseline_path, "w", encoding="utf-8") as fh:
            json.dump(out, fh, indent=2, ensure_ascii=False)
            fh.write("\n")
    except OSError as e:
        print(f"FAIL: cannot write baseline {baseline_path}: {e}")
        return 2
    print(f"updated: +{added} added, -{dropped} removed; final suite count: {len(kept)}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Freeze-check the test-suite -> claim inventory.")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true",
                      help="verify only (default)")
    mode.add_argument("--update", action="store_true",
                      help="rewrite the baseline's suites")
    parser.add_argument("--baseline", default=None,
                        help="baseline path (default: <root>/docs/test-inventory.json)")
    parser.add_argument("--root", default=None,
                        help="repo root override (default: two levels above this script)")
    args = parser.parse_args()

    root = Path(args.root).resolve() if args.root else Path(__file__).resolve().parent.parent
    baseline_path = (Path(args.baseline) if args.baseline
                     else root / "docs" / "test-inventory.json")

    if not root.is_dir():
        print(f"FAIL: repo root not found: {root}")
        return 2

    try:
        baseline = load_baseline(baseline_path)
    except ValueError as e:
        print(f"FAIL: {e}")
        return 2

    if args.update:
        return update(root, baseline, baseline_path)
    return check(root, baseline)


if __name__ == "__main__":
    sys.exit(main())
