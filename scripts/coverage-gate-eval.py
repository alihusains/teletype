#!/usr/bin/env python3
"""Evaluator for scripts/coverage-gate.sh.

Reads `cargo llvm-cov --workspace --json --summary-only` on stdin (one JSON
object per line) and fails when any tracked crate's line coverage is below
its floor. Args: CORE_MIN SPEECH_MIN DESKTOP_MIN (percentages).
"""
import json
import sys

core_min, speech_min, desktop_min = (float(x) for x in sys.argv[1:4])
thresholds = {
    "teletype-core": core_min,
    "teletype-speech": speech_min,
    "teletype-desktop": desktop_min,
}

per_crate = {}
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        obj = json.loads(line)
    except json.JSONDecodeError:
        continue
    # cargo-llvm-cov --json wraps llvm-cov's json: each entry has "files".
    files = obj.get("files") or []
    for f in files:
        rel = f.get("file") or ""
        if "/crates/teletype-" not in rel:
            continue
        crate = rel.split("/crates/", 1)[1].split("/", 1)[0]
        cov = f.get("summary", {}).get("lines", {})
        total, covered = cov.get("total", 0), cov.get("covered", 0)
        if not total:
            continue
        cur = per_crate.setdefault(crate, [0, 0])
        cur[0] += total
        cur[1] += covered

failures = []
print(f"{'crate':<20} {'lines':>8} {'coverage':>10} {'min':>6}")
for crate, (total, covered) in sorted(per_crate.items()):
    if crate not in thresholds:
        continue
    pct = 100.0 * covered / total if total else 0.0
    bar = "OK  " if pct >= thresholds[crate] else "FAIL"
    print(f"{crate:<20} {total:>8} {pct:>9.2f}% {thresholds[crate]:>5.0f}  {bar}")
    if pct < thresholds[crate]:
        failures.append((crate, pct, thresholds[crate]))

if failures:
    for crate, pct, mn in failures:
        print(f"coverage-gate: {crate} at {pct:.2f}% is below the {mn:.0f}% floor", file=sys.stderr)
    sys.exit(1)
print("coverage-gate: all crates at or above their floors")
