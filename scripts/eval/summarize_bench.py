#!/usr/bin/env python3
"""Summarise a benchmark run tree into one comparison table.

Reads every `report.json` under a run root and writes `SUMMARY.md` plus
`summary.json` next to it:

    evals/runs/bench-2026-10-21/
      baseline/<engine>/<set>-off/report.json      ← the models we ship
      candidates/<candidate>/<set>-off/report.json ← external candidates

Every report was produced by the same Rust harness (`teletype-eval`), so the
numbers are directly comparable. Two WER figures are reported:

- **verbatim** — `wer_verbatim_micro` (normalized, fillers dropped). The
  metric the release gate uses.
- **pooled legacy** — `total_errors / total_reference_words` summed over the
  sets, i.e. one corpus-level rate rather than a mean of rates.

Latency is the per-clip ASR time the harness recorded (`latency_asr`); the
candidates' real-time factor comes from the Python bencher's `.meta.json`.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

# Engine/candidate display order: shipping engines first (best first), then
# the external candidates.
GROUP_ORDER = ["baseline", "candidates"]


def load_cells(root: Path):
    """Return {group: {name: {set: report}}}."""
    groups: dict[str, dict[str, dict[str, dict]]] = {}
    for group in GROUP_ORDER:
        gdir = root / group
        if not gdir.is_dir():
            continue
        for name_dir in sorted(gdir.iterdir()):
            if not name_dir.is_dir():
                continue
            cells = {}
            for report_path in sorted(name_dir.glob("*/report.json")):
                set_name = report_path.parent.name
                # Cell dirs are `<set>-off` / `<set>-on`; the boost axis is
                # stripped so the table shows one column per set.
                set_name = set_name.rsplit("-", 1)[0]
                try:
                    cells[set_name] = json.loads(report_path.read_text())
                except json.JSONDecodeError as exc:
                    print(f"warning: skipping {report_path}: {exc}")
            if cells:
                groups.setdefault(group, {})[name_dir.name] = cells
    return groups


def pooled_legacy(cells: dict[str, dict]) -> tuple[float | None, int, int]:
    """(micro WER, total errors, total reference words) pooled over the sets."""
    errors = 0
    words = 0
    for report in cells.values():
        wer = report.get("wer") or {}
        errors += wer.get("total_errors", 0)
        words += wer.get("total_reference_words", 0)
    if words == 0:
        return None, 0, 0
    return errors / words, errors, words


def weighted_verbatim(cells: dict[str, dict]) -> float | None:
    """Verbatim WER weighted by reference words (a pooled rate)."""
    num = 0.0
    den = 0
    for report in cells.values():
        rate = report.get("wer_verbatim_micro")
        words = (report.get("wer") or {}).get("total_reference_words", 0)
        if rate is None or words == 0:
            continue
        num += rate * words
        den += words
    return (num / den) if den else None


def latency(cells: dict[str, dict]) -> tuple[float | None, float | None]:
    """(p95 ASR ms pooled over cases, mean ASR ms pooled over cases)."""
    p95_sum = 0.0
    mean_sum = 0.0
    count = 0
    for report in cells.values():
        lat = report.get("latency_asr")
        if not lat or not lat.get("count"):
            continue
        n = lat["count"]
        p95_sum += lat["p95"] * n
        mean_sum += lat["mean"] * n
        count += n
    if count == 0:
        return None, None
    return p95_sum / count, mean_sum / count


def candidate_rtf(root: Path, candidate: str) -> float | None:
    """Real-time factor from the Python bencher's per-set meta files."""
    values = []
    for meta_path in (root / "candidates" / candidate).glob("*/hypotheses.meta.json"):
        try:
            meta = json.loads(meta_path.read_text())
        except json.JSONDecodeError:
            continue
        if meta.get("rtf") is not None:
            values.append((meta["model_seconds"], meta["audio_seconds"]))
    total_model = sum(v[0] for v in values)
    total_audio = sum(v[1] for v in values)
    if total_audio <= 0:
        return None
    return total_model / total_audio


def fmt(value, spec="%.4f", dash="—"):
    if value is None:
        return dash
    return spec % value


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, help="benchmark run root")
    parser.add_argument("--out", default=None, help="summary path (default <root>/SUMMARY.md)")
    args = parser.parse_args()

    root = Path(args.root)
    groups = load_cells(root)
    if not groups:
        raise SystemExit(f"no reports under {root}")

    sets: list[str] = []
    for group in groups.values():
        for cells in group.values():
            for set_name in cells:
                if set_name not in sets:
                    sets.append(set_name)
    sets.sort()

    rows = []
    for group, entries in groups.items():
        for name, cells in entries.items():
            pooled, errors, words = pooled_legacy(cells)
            verbatim = weighted_verbatim(cells)
            p95, mean = latency(cells)
            cases = sum((r.get("wer") or {}).get("cases", 0) for r in cells.values())
            failed = sum(len(r.get("case_errors") or []) for r in cells.values())
            rows.append(
                {
                    "group": group,
                    "name": name,
                    "cells": cells,
                    "pooled": pooled,
                    "errors": errors,
                    "words": words,
                    "verbatim": verbatim,
                    "p95": p95,
                    "mean": mean,
                    "cases": cases,
                    "failed": failed,
                    "rtf": candidate_rtf(root, name) if group == "candidates" else None,
                }
            )

    lines = []
    lines.append("# ASR benchmark — shipping engines vs external candidates")
    lines.append("")
    lines.append(f"Run root: `{root}`")
    lines.append("")
    lines.append(
        "Every row was scored by the same Rust harness (`teletype-eval`), so the "
        "numbers are directly comparable. Boost axis: off for all cells."
    )
    lines.append("")

    # ── per-set verbatim WER ────────────────────────────────────────────
    lines.append("## Verbatim WER per set (lower is better)")
    lines.append("")
    header = "| engine | " + " | ".join(sets) + " | pooled | cases |"
    lines.append(header)
    lines.append("|" + "---|" * (len(sets) + 3))
    for row in rows:
        cells_md = []
        for set_name in sets:
            report = row["cells"].get(set_name)
            if report is None:
                cells_md.append("—")
            else:
                cells_md.append(fmt(report.get("wer_verbatim_micro"), "%.3f"))
        lines.append(
            f"| `{row['name']}` | "
            + " | ".join(cells_md)
            + f" | {fmt(row['verbatim'], '%.4f')} | {row['cases']} |"
        )
    lines.append("")

    # ── summary table ───────────────────────────────────────────────────
    lines.append("## Summary")
    lines.append("")
    lines.append(
        "| engine | group | pooled legacy WER | verbatim WER | errors/words | "
        "ASR p95 (ms) | ASR mean (ms) | RTF | failed cases |"
    )
    lines.append("|" + "---|" * 9)
    for row in sorted(rows, key=lambda r: (r["pooled"] is None, r["pooled"])):
        lines.append(
            f"| `{row['name']}` | {row['group']} | "
            f"{fmt(row['pooled'], '%.4f')} | {fmt(row['verbatim'], '%.4f')} | "
            f"{row['errors']}/{row['words']} | "
            f"{fmt(row['p95'], '%.0f')} | {fmt(row['mean'], '%.0f')} | "
            f"{fmt(row['rtf'], '%.4f')} | {row['failed']} |"
        )
    lines.append("")
    lines.append(
        "*pooled legacy WER* = total errors / total reference words summed over "
        "the sets; *verbatim WER* = the normalized rate weighted by reference "
        "words. Latency is per-clip ASR time on this host (Apple M4 Pro, macOS, "
        "Metal for the ggml engines). RTF is only defined for the Python-"
        "run candidates (audio seconds / model seconds)."
    )
    lines.append("")

    out_path = Path(args.out) if args.out else root / "SUMMARY.md"
    out_path.write_text("\n".join(lines), encoding="utf-8")
    (root / "summary.json").write_text(
        json.dumps(
            {
                "root": str(root),
                "sets": sets,
                "rows": [
                    {k: v for k, v in row.items() if k != "cells"} for row in rows
                ],
            },
            indent=2,
            default=str,
        ),
        encoding="utf-8",
    )
    print("\n".join(lines))
    print(f"\nwrote {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
