#!/usr/bin/env python3
"""Ratchet gate for the NeuDecide command suite.

Compares a fresh `teletype-command-suite-report-v1` against the committed
baseline `evals/command-suite-baseline.json` and fails when a metric drops
beyond `--slack` (default 0.01, one percentage point).

Exit codes:
  0  pass (with optional RATCHET notes for improvements of >= 0.001)
  1  regression beyond slack, or a refused --update-baseline
  2  usage error
  3  not comparable: bad schema, missing/unparseable report, missing baseline,
     corpus-fingerprint (manifest_sha256_12) mismatch, missing baseline
     category, missing required report fields, or zero scored clips

Rules:
  * The report's `manifest_sha256_12` must equal the baseline's; otherwise the
    corpora differ and the comparison is refused (exit 3).
  * Overall and per-category accuracy may not drop below baseline minus slack.
  * `--update-baseline` refuses to write if ANY metric would drop below
    current minus slack (no silent lowering); the first seed always writes.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import tempfile

REPORT_SCHEMA = "teletype-command-suite-report-v1"
BASELINE_SCHEMA = "teletype-command-suite-baseline-v1"
DEFAULT_BASELINE = "evals/command-suite-baseline.json"
DEFAULT_SLACK = 0.01
IMPROVE_EPS = 0.001
SCORED_MIN = 10  # only categories scored >= this many at baseline time are gated


def fail_not_comparable(msg: str) -> int:
    print(f"NOT COMPARABLE: {msg}", file=sys.stderr)
    return 3


def is_number(x: object) -> bool:
    return isinstance(x, (int, float)) and not isinstance(x, bool)


def validate_report(report: dict) -> str | None:
    """Return a reason string if the report is unusable, else None."""
    if report.get("schema") != REPORT_SCHEMA:
        return f"report schema is {report.get('schema')!r}, expected {REPORT_SCHEMA!r}"
    manifest = report.get("manifest_sha256_12")
    if not isinstance(manifest, str):
        return "report is missing a string `manifest_sha256_12`"
    agg = report.get("aggregates")
    if not isinstance(agg, dict):
        return "report is missing the `aggregates` object"
    overall = agg.get("overall")
    if not isinstance(overall, dict):
        return "report is missing `aggregates.overall`"
    neu = overall.get("neudecide")
    if not isinstance(neu, dict) or not is_number(neu.get("accuracy")):
        return "report is missing a numeric `aggregates.overall.neudecide.accuracy`"
    scored = neu.get("scored")
    if not is_number(scored) or scored <= 0:
        return (
            "report has no scored clips "
            "(`aggregates.overall.neudecide.scored` must be > 0)"
        )
    per_category = agg.get("per_category")
    if not isinstance(per_category, dict):
        return "report is missing the `aggregates.per_category` object"
    return None


def candidate_baseline(report: dict, source_report: str) -> dict:
    """Build a baseline candidate from a validated report.

    Warns (to stderr) for categories whose `neudecide` aggregate is null.
    """
    agg = report["aggregates"]
    overall = agg["overall"]["neudecide"]
    per_category = agg["per_category"]

    acc: dict[str, float] = {}
    scored: dict[str, int] = {}
    for cat in per_category:
        entry = per_category[cat]
        neu = entry.get("neudecide") if isinstance(entry, dict) else None
        if not isinstance(neu, dict) or not is_number(neu.get("accuracy")):
            print(f"warning: category {cat!r} has no usable neudecide aggregate; omitting", file=sys.stderr)
            continue
        acc[cat] = float(neu["accuracy"])
        sc = neu.get("scored")
        scored[cat] = int(sc) if isinstance(sc, (int, float)) else 0

    return {
        "schema": BASELINE_SCHEMA,
        "source_report": source_report,
        "source_date": report.get("date"),
        "manifest_sha256_12": report["manifest_sha256_12"],
        "clips": report.get("clips"),
        "overall_accuracy": float(overall["accuracy"]),
        "per_category_accuracy": acc,
        "per_category_scored": scored,
    }


def sha256_of(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def fmt(x: object) -> str:
    return f"{x:.4f}" if is_number(x) else repr(x)


def write_baseline(path: str, baseline: dict) -> None:
    text = json.dumps(baseline, indent=2, sort_keys=True) + "\n"
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


def load_baseline(path: str) -> dict | None:
    try:
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
    except FileNotFoundError:
        return None
    except (json.JSONDecodeError, OSError) as e:
        print(f"baseline at {path} is unreadable: {e}", file=sys.stderr)
        return None
    if not isinstance(data, dict):
        print(f"baseline at {path} is not a JSON object", file=sys.stderr)
        return None
    return data


def _overall_failure(overall: float, base: float, slack: float) -> str | None:
    if overall < base - slack:
        return f"overall {fmt(overall)} below baseline {fmt(base)} (delta {overall - base:+.4f}, slack {slack})"
    return None


def _overall_ratchet(overall: float, base: float) -> str | None:
    if overall >= base + IMPROVE_EPS:
        return f"RATCHET: overall {fmt(base)} -> {fmt(overall)} (run --update-baseline to lock it in)"
    return None


def _cat_failure(cat: str, cur: float, base: float, slack: float) -> str | None:
    if cur < base - slack:
        return f"{cat} {fmt(cur)} below baseline {fmt(base)} (delta {cur - base:+.4f})"
    return None


def _cat_ratchet(cat: str, cur: float, base: float) -> str | None:
    if cur >= base + IMPROVE_EPS:
        return f"RATCHET: {cat} {fmt(base)} -> {fmt(cur)} (run --update-baseline to lock it in)"
    return None


def gate(report: dict, baseline: dict, slack: float) -> int:
    manifest = report["manifest_sha256_12"]
    base_manifest = baseline.get("manifest_sha256_12")
    if base_manifest != manifest:
        print(f"  report:   {manifest}")
        print(f"  baseline: {base_manifest}")
        return fail_not_comparable("corpus fingerprint differs")

    overall = float(report["aggregates"]["overall"]["neudecide"]["accuracy"])
    base_overall_raw = baseline.get("overall_accuracy")
    failures: list[str] = []
    ratchets: list[str] = []

    if is_number(base_overall_raw):
        base_overall = float(base_overall_raw)
        m = _overall_failure(overall, base_overall, slack)
        if m is not None:
            failures.append(m)
        else:
            r = _overall_ratchet(overall, base_overall)
            if r is not None:
                ratchets.append(r)

    base_cat_acc = baseline.get("per_category_accuracy") or {}
    base_cat_scored = baseline.get("per_category_scored") or {}
    per_category = report["aggregates"]["per_category"]
    for cat in sorted(base_cat_acc):
        base_acc_raw = base_cat_acc[cat]
        scored_raw = base_cat_scored.get(cat)
        if not is_number(base_acc_raw):
            continue
        if not (isinstance(scored_raw, (int, float)) and scored_raw >= SCORED_MIN):
            continue
        base_acc = float(base_acc_raw)
        entry = per_category.get(cat)
        neu = entry.get("neudecide") if isinstance(entry, dict) else None
        cur_raw = neu.get("accuracy") if isinstance(neu, dict) else None
        if not is_number(cur_raw):
            return fail_not_comparable(f"baseline category {cat!r} is missing from the report")
        cur = float(cur_raw)
        m = _cat_failure(cat, cur, base_acc, slack)
        if m is not None:
            failures.append(m)
        else:
            r = _cat_ratchet(cat, cur, base_acc)
            if r is not None:
                ratchets.append(r)

    for line in ratchets:
        print(line)
    if failures:
        print(f"REGRESSION: {len(failures)} metric(s) below baseline minus slack:", file=sys.stderr)
        for line in failures:
            print(f"  {line}", file=sys.stderr)
        return 1
    print("gate: PASS (command suite at or above baseline)")
    return 0


def update_baseline(report: dict, baseline_path: str, source_report: str, slack: float) -> int:
    candidate = candidate_baseline(report, source_report)
    current = load_baseline(baseline_path)
    old_sha = sha256_of(baseline_path) if os.path.exists(baseline_path) else None

    if isinstance(current, dict):
        drops: list[str] = []
        base_overall_raw = current.get("overall_accuracy")
        if is_number(base_overall_raw):
            base_overall = float(base_overall_raw)
            cur_overall = float(candidate["overall_accuracy"])
            if cur_overall < base_overall - slack:
                drops.append(
                    f"overall {fmt(cur_overall)} below current {fmt(base_overall)} "
                    f"(delta {cur_overall - base_overall:+.4f})"
                )
        base_cat_acc = current.get("per_category_accuracy") or {}
        for cat in sorted(base_cat_acc):
            base_acc_raw = base_cat_acc[cat]
            cur_acc_raw = candidate["per_category_accuracy"].get(cat)
            if not is_number(base_acc_raw):
                continue
            base_acc = float(base_acc_raw)
            if not is_number(cur_acc_raw):
                drops.append(f"{cat} dropped out of the report (current {fmt(base_acc)})")
            else:
                cur_acc = float(cur_acc_raw)
                if cur_acc < base_acc - slack:
                    drops.append(
                        f"{cat} {fmt(cur_acc)} below current {fmt(base_acc)} "
                        f"(delta {cur_acc - base_acc:+.4f})"
                    )
        if drops:
            print(f"REFUSED: --update-baseline would lower {len(drops)} metric(s); not writing:", file=sys.stderr)
            for line in drops:
                print(f"  {line}", file=sys.stderr)
            return 1

    write_baseline(baseline_path, candidate)
    new_sha = sha256_of(baseline_path)
    if old_sha is None:
        print(f"seeded baseline: {baseline_path}")
        print(f"  new sha256: {new_sha}")
    else:
        print(f"updated baseline: {baseline_path}")
        print(f"  old sha256: {old_sha}")
        print(f"  new sha256: {new_sha}")
    return 0


# ---------------------------------------------------------------------------
# self-test
# ---------------------------------------------------------------------------

def _write(path: str, obj: dict) -> None:
    with open(path, "w", encoding="utf-8") as f:
        json.dump(obj, f)


def _report(manifest: str, overall: float, cats: dict[str, tuple[int, int]]) -> dict:
    per_category = {}
    for cat, (correct, scored) in cats.items():
        per_category[cat] = {
            "neudecide": {"correct": correct, "scored": scored, "accuracy": (correct / scored) if scored else 0.0},
            "needle": None,
            "llm": None,
        }
    return {
        "schema": REPORT_SCHEMA,
        "manifest_sha256_12": manifest,
        "date": "2099-01-01",
        "clips": sum(s for _, s in cats.values()),
        "aggregates": {
            "overall": {"neudecide": {"correct": int(overall * 100), "scored": 100, "accuracy": overall}, "needle": None, "llm": None},
            "per_category": per_category,
        },
    }


def _run(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--report")
    parser.add_argument("--baseline", default=DEFAULT_BASELINE)
    parser.add_argument("--update-baseline", action="store_true")
    parser.add_argument("--slack", type=float, default=DEFAULT_SLACK)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    try:
        return main(args)
    except SystemExit as e:
        # `load_baseline_or_die` exits 3 by design; the self-test observes
        # exit codes in-process, so translate the exit into a return value.
        if isinstance(e.code, int):
            return e.code
        return 1 if e.code else 0


def self_test() -> int:
    with tempfile.TemporaryDirectory(prefix="cs-gate-") as tmp:
        baseline = os.path.join(tmp, "baseline.json")

        def make(name: str, manifest: str = "man-aaaa", overall: float = 0.84,
                 cats: dict[str, tuple[int, int]] | None = None) -> str:
            if cats is None:
                cats = {"a": (30, 30), "b": (30, 30)}
            p = os.path.join(tmp, name)
            _write(p, _report(manifest, overall, cats))
            return p

        r1 = make("r1.json")

        # 1. no baseline -> 3
        assert _run(["--report", r1, "--baseline", baseline]) == 3, "missing baseline must be exit 3"
        # 2. seed -> 0, file written
        assert _run(["--update-baseline", "--report", r1, "--baseline", baseline]) == 0, "seed must write"
        with open(baseline, encoding="utf-8") as f:
            bl = json.load(f)
        assert bl["schema"] == BASELINE_SCHEMA
        assert bl["manifest_sha256_12"] == "man-aaaa"
        assert bl["overall_accuracy"] == 0.84
        assert bl["per_category_accuracy"] == {"a": 1.0, "b": 1.0}
        assert bl["per_category_scored"] == {"a": 30, "b": 30}
        # 3. identical recheck -> 0
        assert _run(["--report", r1, "--baseline", baseline]) == 0, "identical report must pass"

        # 4. overall regression -> 1
        rp = make("reg.json", overall=0.50)
        assert _run(["--report", rp, "--baseline", baseline]) == 1, "regression must be exit 1"

        # 5. corpus mismatch -> 3
        cp = make("corp.json", manifest="man-bbbb")
        assert _run(["--report", cp, "--baseline", baseline]) == 3, "corpus mismatch must be exit 3"

        # 6. missing baseline category -> 3
        mp = make("miss.json", cats={"a": (30, 30)})
        assert _run(["--report", mp, "--baseline", baseline]) == 3, "missing category must be exit 3"

        # 7. bad schema -> 3
        bad = _report("man-aaaa", 0.84, {"a": (30, 30), "b": (0, 30)})
        bad["schema"] = "wrong"
        bp = os.path.join(tmp, "bad.json")
        _write(bp, bad)
        assert _run(["--report", bp, "--baseline", baseline]) == 3, "bad schema must be exit 3"

        # 8. refused update (regressed report would lower overall) -> 1, file unchanged
        sha_before = sha256_of(baseline)
        assert _run(["--update-baseline", "--report", rp, "--baseline", baseline]) == 1, "refused update must be exit 1"
        assert sha256_of(baseline) == sha_before, "refused update must not write"

        # 9. category regression beyond slack -> 1
        lp = make("lowb.json", cats={"a": (30, 30), "b": (20, 30)})
        assert _run(["--report", lp, "--baseline", baseline]) == 1, "category drop must be exit 1"

        # 10. slack honored: drop within slack passes (99/100 drops exactly 0.01)
        op = make("okb.json", cats={"a": (30, 30), "b": (99, 100)})
        assert _run(["--report", op, "--baseline", baseline]) == 0, "drop within slack must pass"

        # 11. low-scored baseline category is not gated: seed a baseline where
        # category `c` has scored < 10, then a report dropping `c` still passes.
        lowp = make("low.json", manifest="man-cccc", overall=0.80, cats={"c": (0, 5)})
        lowbl = os.path.join(tmp, "lowbl.json")
        assert _run(["--update-baseline", "--report", lowp, "--baseline", lowbl]) == 0
        ld = make("lowdrop.json", manifest="man-cccc", overall=0.80, cats={"c": (1, 5)})
        assert _run(["--report", ld, "--baseline", lowbl]) == 0, "low-scored category must not be gated"

        # 12. category improvement prints a RATCHET line and still passes
        imp = make("imp.json", overall=0.90)
        assert _run(["--report", imp, "--baseline", baseline]) == 0, "improvement must pass"

        # 13. zero scored clips -> 3: a report that measured nothing must not
        # compare, pass, or seed a baseline. This is the legacy-binary path
        # where the skip notes never reached the gate.
        zp = make("zero.json")
        with open(zp, encoding="utf-8") as f:
            zero = json.load(f)
        zero["aggregates"]["overall"]["neudecide"]["scored"] = 0
        _write(zp, zero)
        assert _run(["--report", zp, "--baseline", baseline]) == 3, \
            "zero scored clips must be exit 3"
        never = os.path.join(tmp, "never.json")
        assert _run(["--update-baseline", "--report", zp, "--baseline", never]) == 3, \
            "zero scored clips must not seed a baseline"
        assert not os.path.exists(never), "refused seed must not write a baseline file"

    print("SELF-TEST: PASS")
    return 0


# ---------------------------------------------------------------------------

def main(args: argparse.Namespace) -> int:
    if not args.report:
        print("usage error: --report is required (or use --self-test)", file=sys.stderr)
        return 2
    if not os.path.exists(args.report):
        print(f"usage error: report not found: {args.report}", file=sys.stderr)
        return 2
    try:
        with open(args.report, encoding="utf-8") as f:
            report = json.load(f)
    except (json.JSONDecodeError, OSError) as e:
        return fail_not_comparable(f"cannot parse report {args.report}: {e}")
    if not isinstance(report, dict):
        return fail_not_comparable("report is not a JSON object")

    reason = validate_report(report)
    if reason is not None:
        return fail_not_comparable(reason)

    if args.update_baseline:
        return update_baseline(report, args.baseline, args.report, args.slack)
    return gate(report, load_baseline_or_die(args.baseline), args.slack)


def load_baseline_or_die(path: str) -> dict:
    baseline = load_baseline(path)
    if baseline is None:
        print("no baseline recorded; run --update-baseline --report <report>")
        sys.exit(3)
    return baseline


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        description="Ratchet gate for the NeuDecide command suite report."
    )
    p.add_argument("--report", help="path to a command-suite report.json")
    p.add_argument("--baseline", default=DEFAULT_BASELINE, help=f"baseline path (default {DEFAULT_BASELINE})")
    p.add_argument("--update-baseline", action="store_true", help="rewrite the baseline from --report")
    p.add_argument("--slack", type=float, default=DEFAULT_SLACK, help="allowed absolute drop per metric (default 0.01)")
    p.add_argument("--self-test", action="store_true", help="run built-in fixtures and exit")
    return p


if __name__ == "__main__":
    parser = build_parser()
    ns = parser.parse_args()
    if ns.self_test:
        sys.exit(self_test())
    if not ns.report and not ns.update_baseline:
        print("usage error: --report is required (or use --self-test)", file=sys.stderr)
        sys.exit(2)
    sys.exit(main(ns))
