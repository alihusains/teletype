#!/usr/bin/env python3
"""Head-to-head dictation benchmark: Teletype vs Wispr Flow (plan §10.1).

For each (app, clip, pass) the script:
  1. focuses TextEdit and clears its document (osascript),
  2. holds the app's dictation hotkey chord (System Events key down/up),
  3. plays the clip's WAV with afplay (blocking),
  4. releases the chord and stamps t_release,
  5. polls TextEdit's document text every 25 ms, stamping t_text at the
     first change,
  6. waits --settle seconds and reads the final text.

Per-take rows go to <out>/takes.csv:
  app, clip, pass, t_release, t_text, release_to_text_ms, final_text,
  expected_text, failure

`--metrics DIR` recomputes p50/p95/p99 of release_to_text_ms, WER vs
<clip>.txt, and failure rate from an existing takes.csv.

`--demo` runs the whole metrics/CSV pipeline on 20 synthetic rows and
must run green with no apps, mics, or permissions.
"""

import argparse
import csv
import json
import os
import random
import subprocess
import sys
import time

POLL_S = 0.025
FAILURE_S = 5.0

DEFAULT_HOTKEYS = {"teletype": "ctrl+cmd+space", "wispr": "shift+shift"}

WISPR_TEMPLATE = """# Teletype vs Wispr Flow — head-to-head comparison

Raw data: `takes.csv` (this directory). Metrics below are regenerated
with `python3 scripts/bench/h2h.py --metrics <this-dir>`.

## Methodology

- Target app: TextEdit, document cleared before every take.
- Hotkey held, clip WAV played via afplay, chord released; t_release
  stamped at release. Observer polls TextEdit text every 25 ms; t_text
  stamped at the first change. final_text read after {settle} s settle.
- {passes} passes per clip per app. Failure = no text within 5 s.
- WER computed against <clip>.txt (unit Levenshtein edit distance).

## Versions

| component | version |
|---|---|
| macOS | FILL |
| Teletype | FILL (git sha) |
| Wispr Flow | FILL (app version) |
| Mic / input mode | FILL (real mic via virtual mic, e.g. BlackHole) |

## Results

FILL from the metrics run: p50/p95/p99 release_to_text_ms, WER,
failure rate, per app. Link chart images here when generated.

## Raw data

See takes.csv. One row per take.
"""


def parse_args(argv):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--app", action="append", choices=["teletype", "wispr"],
                   help="app to benchmark (repeatable; default both)")
    p.add_argument("--corpus", help="corpus dir with <clip>.wav + <clip>.txt")
    p.add_argument("--passes", type=int, default=3)
    p.add_argument("--out", default="bench-out", help="output dir")
    p.add_argument("--hotkey", action="append", default=None,
                   metavar="APP=CHORD",
                   help='override a hotkey, e.g. --hotkey teletype="ctrl+cmd+space"')
    p.add_argument("--settle", type=float, default=3.0,
                   help="seconds to wait before reading final text")
    p.add_argument("--demo", action="store_true",
                   help="run the pipeline on synthetic data (no hardware)")
    p.add_argument("--metrics", metavar="DIR",
                   help="recompute metrics from an existing takes.csv in DIR")
    return p.parse_args(argv)


# --- metrics ----------------------------------------------------------------

def edit_distance(a, b):
    if a == b:
        return 0
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i]
        for j, cb in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1,
                           prev[j - 1] + (ca != cb)))
        prev = cur
    return prev[-1]


def wer(ref, hyp):
    ref_words = ref.split()
    if not ref_words:
        return 0.0
    return edit_distance(ref_words, hyp.split()) / len(ref_words)


def percentile(sorted_vals, pct):
    if not sorted_vals:
        return None
    k = (len(sorted_vals) - 1) * pct / 100.0
    lo, hi = int(k), min(int(k) + 1, len(sorted_vals) - 1)
    frac = k - lo
    return sorted_vals[lo] * (1 - frac) + sorted_vals[hi] * frac


def load_rows(csv_path):
    with open(csv_path, newline="") as f:
        return list(csv.DictReader(f))


def compute_metrics(rows):
    per_app = {}
    for row in rows:
        per_app.setdefault(row["app"], []).append(row)
    out = {}
    for app, app_rows in sorted(per_app.items()):
        lat = sorted(float(r["release_to_text_ms"]) for r in app_rows
                     if r["failure"] != "1" and r["release_to_text_ms"])
        fails = sum(1 for r in app_rows if r["failure"] == "1")
        wers = [wer(r["expected_text"], r["final_text"]) for r in app_rows]
        out[app] = {
            "takes": len(app_rows),
            "p50_ms": percentile(lat, 50),
            "p95_ms": percentile(lat, 95),
            "p99_ms": percentile(lat, 99),
            "mean_wer": sum(wers) / len(wers) if wers else None,
            "failure_rate": fails / len(app_rows),
        }
    return out


def fmt_metrics(metrics):
    lines = ["app\ttakes\tp50_ms\tp95_ms\tp99_ms\tmean_wer\tfailure_rate"]
    for app, m in metrics.items():
        def f(v):
            return "n/a" if v is None else f"{v:.3f}"
        lines.append("\t".join([app, str(m["takes"]), f(m["p50_ms"]),
                                f(m["p95_ms"]), f(m["p99_ms"]),
                                f(m["mean_wer"]), f"{m['failure_rate']:.3f}"]))
    return "\n".join(lines)


# --- demo -------------------------------------------------------------------

DEMO_TEXTS = [
    "The quarterly numbers look better than expected",
    "Please send the updated contract to the client today",
    "Ship the release after the QA pass is green",
    "Book a room for the architecture review next week",
]


def demo_rows(out_dir):
    rng = random.Random(42)
    rows = []
    for app in ("teletype", "wispr"):
        for i in range(10):
            clip = f"demo-{i + 1:02d}"
            expected = DEMO_TEXTS[i % len(DEMO_TEXTS)]
            final = expected
            if rng.random() < 0.15:
                final = expected.replace("the", "teh")
            failure = rng.random() < 0.05
            t_release = time.monotonic()
            rt = rng.uniform(150, 1400) if not failure else None
            rows.append({
                "app": app, "clip": clip, "pass": 1,
                "t_release": f"{t_release:.6f}",
                "t_text": f"{t_release + rt / 1000:.6f}" if rt is not None else "",
                "release_to_text_ms": f"{rt:.1f}" if rt is not None else "",
                "final_text": final, "expected_text": expected,
                "failure": "1" if failure else "0",
            })
    return rows


def write_rows(path, rows):
    if not rows:
        return
    fields = list(rows[0].keys())
    with open(path, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields)
        w.writeheader()
        w.writerows(rows)


def write_report(out_dir, metrics, settle, passes):
    with open(os.path.join(out_dir, "metrics.txt"), "w") as f:
        f.write(fmt_metrics(metrics) + "\n")
    with open(os.path.join(out_dir, "WISPR_COMPARISON.md"), "w") as f:
        f.write(WISPR_TEMPLATE.format(settle=settle, passes=passes))


# --- live automation ----------------------------------------------------------

def osascript(script):
    return subprocess.run(["osascript", "-e", script],
                          capture_output=True, text=True)


def focus_textedit():
    r = osascript(
        'tell application "TextEdit" to activate\n'
        'tell application "System Events" to tell process "TextEdit"\n'
        '  keystroke "a" using {command down}\n'
        '  key code 51\n'
        '  keystroke "n" using {command down}\n'
        "end tell")
    if r.returncode != 0:
        raise RuntimeError(f"focus/clear TextEdit failed: {r.stderr.strip()}")


def chord_parts(hotkey):
    mods, key = [], None
    for part in hotkey.lower().split("+"):
        if part in ("ctrl", "cmd", "command", "shift", "option", "alt"):
            mods.append(part)
        else:
            key = part
    if key is None:
        raise ValueError(f"hotkey {hotkey!r} has no key")
    return mods, key


MOD_TOKENS = {"ctrl": "control", "cmd": "command", "command": "command",
              "shift": "shift", "option": "option", "alt": "option"}


def hold_key(hotkey):
    """Press and hold the chord (modifiers first, then the key)."""
    mods, key = chord_parts(hotkey)
    if mods:
        down = ", ".join(f"{MOD_TOKENS[m]} down" for m in mods)
        r = osascript(f'tell application "System Events" to key down {down}')
        if r.returncode != 0:
            return r
    return osascript(f'tell application "System Events" to key down {key!r}')


def release_key(hotkey):
    """Release the chord (key first, then modifiers)."""
    mods, key = chord_parts(hotkey)
    r = osascript(f'tell application "System Events" to key up {key!r}')
    if mods:
        up = ", ".join(f"{MOD_TOKENS[m]} up" for m in mods)
        r2 = osascript(f'tell application "System Events" to key up {up}')
        if r2.returncode != 0:
            r = r2
    return r


def hold_and_play(hotkey, wav):
    r = hold_key(hotkey)
    if r.returncode != 0:
        raise RuntimeError(f"key down failed: {r.stderr.strip()}")
    subprocess.run(["afplay", wav], check=True)
    release_key(hotkey)


def textedit_document_text():
    r = osascript('tell application "System Events" to tell process "TextEdit"\n'
                  '  set t to value of attribute "AXValue" of text area 1 of '
                  'window 1\n'
                  "end tell")
    if r.returncode != 0:
        return None
    return r.stdout.rstrip("\n")


def run_take(app, hotkey, clip, wav, expected):
    focus_textedit()
    hold_and_play(hotkey, wav)
    t_release = time.monotonic()
    t_text = None
    deadline = time.monotonic() + FAILURE_S
    while time.monotonic() < deadline:
        if textedit_document_text():
            t_text = time.monotonic()
            break
        time.sleep(POLL_S)
    time.sleep(0)  # t_text stamped at first change; now settle
    return t_release, t_text


def live_run(args):
    apps = args.app or ["teletype", "wispr"]
    hotkeys = dict(DEFAULT_HOTKEYS)
    for h in (args.hotkey or []):
        app, chord = h.split("=", 1)
        hotkeys[app] = chord.strip('"\'')
    os.makedirs(args.out, exist_ok=True)
    rows = []
    for app in apps:
        for clip, wav in sorted(
                (c, os.path.join(args.corpus, c + ".wav"))
                for c in os.listdir(args.corpus)
                if c.endswith(".wav")):
            clip = clip[:-4]
            expected = ""
            txt = os.path.join(args.corpus, clip + ".txt")
            if os.path.exists(txt):
                expected = open(txt).read().strip()
            for p in range(1, args.passes + 1):
                print(f"[{app}] {clip} pass {p} ...", flush=True)
                t_release, t_text = run_take(app, hotkeys[app], clip, wav,
                                              expected)
                time.sleep(args.settle)
                final = textedit_document_text() or ""
                failure = t_text is None
                rows.append({
                    "app": app, "clip": clip, "pass": p,
                    "t_release": f"{t_release:.6f}",
                    "t_text": f"{t_text:.6f}" if t_text is not None else "",
                    "release_to_text_ms":
                        f"{(t_text - t_release) * 1000:.1f}"
                        if t_text is not None else "",
                    "final_text": final, "expected_text": expected,
                    "failure": "1" if failure else "0",
                })
                write_rows(os.path.join(args.out, "takes.csv"), rows)
    metrics = compute_metrics(rows)
    write_report(args.out, metrics, args.settle, args.passes)
    print(fmt_metrics(metrics))


def main(argv=None):
    args = parse_args(argv)
    if args.metrics:
        rows = load_rows(os.path.join(args.metrics, "takes.csv"))
        metrics = compute_metrics(rows)
        write_report(args.metrics, metrics, args.settle, args.passes)
        print(fmt_metrics(metrics))
        return 0
    if args.demo:
        os.makedirs(args.out, exist_ok=True)
        rows = demo_rows(args.out)
        write_rows(os.path.join(args.out, "takes.csv"), rows)
        metrics = compute_metrics(rows)
        write_report(args.out, metrics, args.settle, args.passes)
        print(fmt_metrics(metrics))
        return 0
    if not args.corpus:
        print("error: --corpus is required for a live run "
              "(or use --demo / --metrics)", file=sys.stderr)
        return 2
    live_run(args)
    return 0


if __name__ == "__main__":
    sys.exit(main())
