#!/usr/bin/env python3
"""Guided human pass over recorded takes.

Walks a human through evaluating one "take" (a directory with
`manifest.jsonl` + `case-NN.wav`, as produced by the segmentation flow —
see evals/human-corpus/FORMAT.md) and records honest verdicts.

Verdicts per case:
  ok      reference/clean text is right as-is
  asr     ASR (transcription) is wrong on this case
  polish  text is right but would read better with light polish
  audio   the recording itself is bad (noise, truncation, mis-utterance)
  skip    the human could not / chose not to judge this case

Semantics (house rule, same as scripts/soak.sh and
scripts/eval/command_suite_gate.py):

  A run that could not observe what it claims must say INSTRUMENT_INVALID
  and exit 3 — never PASS.

The human is the sensor. The only observable playback contract is the
player's exit code: if the player exits nonzero the case counts as
unobserved. We cannot verify the human actually heard anything — document
that limitation here rather than pretending otherwise.

Judgement verdicts (ok/asr/polish/audio) are VALID observations — a
"bad-asr" verdict is data, not a tool failure. There is NO exit code 1: a
human's negative verdict is data, not a tool failure.

Unobserved (any of these forces INSTRUMENT_INVALID, exit 3):
  - player missing or exited nonzero (playback could not be confirmed)
  - WAV file missing or unreadable by python's `wave` module
  - verdict is `skip`
  - answers file exhausted before every case was covered
  - zero cases in the manifest

Exit codes:
  0  verdict PASS: every case observed, every verdict is a judgement
  2  setup error: bad flags, take dir missing, malformed manifest
  3  verdict INSTRUMENT_INVALID: one or more cases unobserved
     (there is no exit 1 — a human's negative verdict is data, not failure)

Usage:
  python3 scripts/eval/human_pass.py [TAKES_DIR]
      [--take NAME] [--answers FILE] [--no-play] [--out DIR] [--list]

TAKES_DIR defaults to evals/human-corpus/scripted/takes. Without --take,
the newest subdirectory containing a manifest.jsonl is used.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import wave

DEFAULT_TAKES_DIR = "evals/human-corpus/scripted/takes"
VERDICTS = ("ok", "asr", "polish", "audio", "skip")
VALID_JUDGEMENTS = ("ok", "asr", "polish", "audio")
EXPECTED_RATE = 16000


def warn(msg: str) -> None:
    print(f"human_pass: warning: {msg}", file=sys.stderr)


def die_usage(msg: str) -> int:
    print(f"human_pass: {msg}", file=sys.stderr)
    return 2


def newest_take_dir(takes_dir: str) -> str | None:
    """Return the newest subdirectory of takes_dir containing manifest.jsonl."""
    try:
        names = os.listdir(takes_dir)
    except OSError:
        return None
    if not os.path.isdir(takes_dir):
        return None
    candidates = []
    for name in names:
        sub = os.path.join(takes_dir, name)
        if os.path.isdir(sub) and os.path.isfile(os.path.join(sub, "manifest.jsonl")):
            candidates.append((os.path.getmtime(sub), name))
    if not candidates:
        return None
    candidates.sort()
    return os.path.join(takes_dir, candidates[-1][1])


def load_manifest(path: str) -> list[dict]:
    """Read manifest.jsonl. Returns list of entries. Raises on malformed lines."""
    entries = []
    with open(path, encoding="utf-8") as f:
        for lineno, line in enumerate(f, 1):
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except json.JSONDecodeError as e:
                raise ValueError(f"{path}:{lineno}: bad JSON: {e}") from e
            if not isinstance(obj, dict):
                raise ValueError(f"{path}:{lineno}: entry is not an object")
            for field in ("id", "audio", "reference"):
                if field not in obj:
                    raise ValueError(f"{path}:{lineno}: missing required field {field!r}")
            entries.append(obj)
    return entries


def validate_wav(path: str) -> tuple[bool, str | None]:
    """Return (ok, detail). detail is a reason string when ok is False."""
    if not os.path.isfile(path):
        return False, f"WAV missing: {path}"
    try:
        with wave.open(path, "rb") as w:
            rate = w.getframerate()
    except (wave.Error, EOFError, OSError) as e:
        return False, f"WAV unreadable: {path} ({e})"
    if rate != EXPECTED_RATE:
        warn(f"{path}: sample rate {rate} Hz (expected {EXPECTED_RATE} Hz)")
    return True, None


def play_audio(path: str, player: list[str], no_play: bool) -> tuple[bool, str | None]:
    """Play one WAV. Return (ok, detail)."""
    if no_play:
        return True, None
    if not player:
        return False, "no player found (HUMAN_PASS_PLAYER unset and afplay missing)"
    try:
        proc = subprocess.run([*player, path], check=False)
    except (OSError, FileNotFoundError) as e:
        return False, f"player failed to launch ({e})"
    if proc.returncode != 0:
        return False, f"player exited {proc.returncode}"
    return True, None


def read_answers(path: str) -> list[str]:
    with open(path, encoding="utf-8") as f:
        return [line.strip() for line in f if line.strip()]


def prompt_verdict(case_id: str) -> str | None:
    """Prompt on /dev/tty for a verdict. Returns None if tty unavailable."""
    if not os.path.exists("/dev/tty"):
        return None
    try:
        with open("/dev/tty") as tty:
            print(f"\ncase {case_id}:", file=sys.stderr)
            print("  verdict [ok|asr|polish|audio|skip]: ", end="", file=sys.stderr)
            sys.stderr.flush()
            line = tty.readline().strip().lower()
    except OSError:
        return None
    return line or None


def write_summary(out_dir: str, take: str, counts: dict, cases: int,
                  unobserved: list[str], verdict: str) -> str:
    summary_path = os.path.join(out_dir, "summary.json")
    summary = {
        "take": take,
        "cases": cases,
        "verdict": verdict,
        "unobserved": unobserved,
        "counts": counts,
    }
    with open(summary_path, "w", encoding="utf-8") as f:
        json.dump(summary, f, indent=2)
        f.write("\n")
    return summary_path


def print_summary_file(summary_path: str) -> None:
    with open(summary_path, encoding="utf-8") as f:
        print(f.read(), end="")


def list_cases(entries: list[dict], take_dir: str) -> int:
    """Print the id | audio | reference table. Returns 0."""
    if not entries:
        print(f"human_pass: take {take_dir} has 0 cases")
        return 0
    print(f"{'id':<16} {'audio':<20} reference")
    for e in entries:
        ref = str(e.get("reference", ""))
        if len(ref) > 60:
            ref = ref[:57] + "..."
        print(f"{str(e.get('id','')):<16} {str(e.get('audio','')):<20} {ref}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Guided human pass over a recorded take. See module docstring "
                    "for verdict semantics, exit codes, and the instrument-invalid "
                    "rule applied here.",
        epilog="Exit codes: 0 PASS (all observed) | 2 setup error | "
               "3 INSTRUMENT_INVALID (any unobserved). No exit 1 — a human's "
               "negative verdict is data, not a tool failure.",
    )
    ap.add_argument("takes_dir", nargs="?", default=DEFAULT_TAKES_DIR,
                    help=f"directory of takes (default: {DEFAULT_TAKES_DIR})")
    ap.add_argument("--take", default=None,
                    help="take name (subdirectory of takes_dir); default: newest "
                         "subdirectory containing manifest.jsonl")
    ap.add_argument("--answers", default=None,
                    help="file with one verdict per line, consumed in order "
                         "(scripted mode)")
    ap.add_argument("--no-play", action="store_true",
                    help="skip playback (for CI/smoke; playback is not required "
                         "for a verdict in scripted mode)")
    ap.add_argument("--out", default=None,
                    help="output directory for pass-results.jsonl and "
                         "summary.json (default: the take dir)")
    ap.add_argument("--list", action="store_true",
                    help="print the id | audio | reference table and exit 0; "
                         "no prompts, no playback")
    args = ap.parse_args()

    # Resolve take dir
    if args.take:
        take_dir = os.path.join(args.takes_dir, args.take)
        if not os.path.isdir(take_dir):
            return die_usage(f"take dir not found: {take_dir}")
    else:
        take_dir = newest_take_dir(args.takes_dir)
        if take_dir is None:
            return die_usage(f"no take found under {args.takes_dir} "
                             f"(no subdirectory with manifest.jsonl)")

    manifest_path = os.path.join(take_dir, "manifest.jsonl")
    if not os.path.isfile(manifest_path):
        return die_usage(f"manifest not found: {manifest_path}")

    try:
        entries = load_manifest(manifest_path)
    except ValueError as e:
        return die_usage(f"malformed manifest: {e}")

    if args.list:
        return list_cases(entries, take_dir)

    out_dir = args.out or take_dir
    try:
        os.makedirs(out_dir, exist_ok=True)
    except OSError as e:
        return die_usage(f"cannot create output dir {out_dir}: {e}")
    results_path = os.path.join(out_dir, "pass-results.jsonl")
    take_name = os.path.basename(os.path.normpath(take_dir))

    # Load answers (scripted mode)
    answers: list[str] | None = None
    if args.answers:
        if not os.path.isfile(args.answers):
            return die_usage(f"answers file not found: {args.answers}")
        answers = read_answers(args.answers)

    player = os.environ.get("HUMAN_PASS_PLAYER", "").strip()
    player_cmd = [player] if player else (
        ["afplay"] if shutil.which("afplay") else []
    )

    counts = dict.fromkeys(VERDICTS, 0)
    unobserved: list[str] = []
    cases_total = len(entries)

    if cases_total == 0:
        unobserved.append("observed 0 cases (empty manifest)")

    answers_exhausted = False
    try:
        rf = open(results_path, "w", encoding="utf-8")
    except OSError as e:
        return die_usage(f"cannot write results file {results_path}: {e}")
    with rf:
        for idx, entry in enumerate(entries):
            cid = str(entry.get("id", f"case-{idx:02d}"))
            audio_rel = str(entry.get("audio", ""))
            reference = str(entry.get("reference", ""))
            wav_path = os.path.join(take_dir, audio_rel) if audio_rel else ""

            case_record: dict = {
                "id": cid,
                "audio": audio_rel,
                "reference": reference,
                "clean": entry.get("clean", ""),
            }

            # 1. Validate WAV
            wav_ok, wav_detail = validate_wav(wav_path) if wav_path else (False, "no audio field")
            if not wav_ok:
                case_record["verdict"] = None
                case_record["reason"] = wav_detail
                unobserved.append(f"{cid}: {wav_detail}")
                rf.write(json.dumps(case_record) + "\n")
                continue

            # 2. Play (unless --no-play)
            play_ok, play_detail = play_audio(wav_path, player_cmd, args.no_play)
            if not play_ok:
                case_record["verdict"] = None
                case_record["reason"] = play_detail
                unobserved.append(f"{cid}: {play_detail}")
                rf.write(json.dumps(case_record) + "\n")
                continue

            # 3. Get verdict
            if answers is not None:
                if idx >= len(answers):
                    if not answers_exhausted:
                        unobserved.append(f"answers exhausted before case {idx + 1} ({cid})")
                        answers_exhausted = True
                    case_record["verdict"] = None
                    case_record["reason"] = "answers exhausted"
                    rf.write(json.dumps(case_record) + "\n")
                    continue
                verdict = answers[idx].lower()
            else:
                verdict = prompt_verdict(cid)
                if verdict is None:
                    case_record["verdict"] = None
                    case_record["reason"] = "no tty / no answer"
                    unobserved.append(f"{cid}: no tty or empty answer")
                    rf.write(json.dumps(case_record) + "\n")
                    continue

            if verdict not in VERDICTS:
                case_record["verdict"] = None
                case_record["reason"] = f"invalid verdict {verdict!r}"
                unobserved.append(f"{cid}: invalid verdict {verdict!r}")
                rf.write(json.dumps(case_record) + "\n")
                continue

            # 4. Record
            case_record["verdict"] = verdict
            if verdict == "skip":
                unobserved.append(f"{cid}: skipped by human")
            else:
                counts[verdict] += 1
            rf.write(json.dumps(case_record) + "\n")

    # 5. Verdict + summary
    observed = sum(counts.values())
    verdict_out = "INSTRUMENT_INVALID" if unobserved else "PASS"
    rc = 3 if unobserved else 0

    summary_path = write_summary(out_dir, take_name, counts, cases_total,
                                 unobserved, verdict_out)

    # Print human-readable RESULT line (mirroring scripts/soak.sh)
    print()
    print("human_pass summary:")
    try:
        print_summary_file(summary_path)
    except OSError as e:
        print(f"human_pass: could not re-read summary.json: {e}", file=sys.stderr)
    if verdict_out == "INSTRUMENT_INVALID":
        print(f"RESULT: INSTRUMENT INVALID ({unobserved[0]})")
    else:
        print(f"RESULT: PASS ({observed}/{cases_total} observed)")
    return rc


if __name__ == "__main__":
    sys.exit(main())
