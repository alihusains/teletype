#!/usr/bin/env bash
# 500-take soak harness for Teletype dictation through the REAL microphone path.
# Sends the same shapes run_take.sh exercies: a fresh TextEdit document per
# take, the hotkey held via the real holdctrl binary, speech via macOS say.
# Per take it records: index, shape, start and end time, frontmost app, trace
# file, committed_segments, audio_ms, tail_audio_ms, recovery file count
# before and after, Teletype process RSS, pass or fail and a failure reason.
# It never writes dictated text, screen text or field text anywhere: counts
# and booleans only.
# Usage: soak.sh [--takes N] [--seed S] [--out DIR] [--dry-run] [--resume]
set -u
DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
. "$DIR/lib.sh"

TAKES=500
SEED=4242
OUT=""
DRY_RUN=0
RESUME=0

while [ $# -gt 0 ]; do
  case "$1" in
    --takes) TAKES="$2"; shift 2 ;;
    --seed) SEED="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --dry-run) DRY_RUN=1; shift ;;
    --resume) RESUME=1; shift ;;
    *)
      echo "unknown option: $1" >&2
      exit 1
      ;;
  esac
done

TRACES="${TELETYPE_TRACES:-$HOME/Library/Application Support/com.teletype.app/traces}"
# holdctrl lives in the main checkout; a worktree copy is optional. Explicit
# TELETYPE_HOLDCTRL wins, then the local copy, then the main checkout.
HOLDCTRL="${TELETYPE_HOLDCTRL:-$DIR/holdctrl}"
if [ -z "${TELETYPE_HOLDCTRL:-}" ] && [ ! -e "$HOLDCTRL" ]; then
  HOLDCTRL="/Users/a.sorathiya/Documents/Ali/teletype/tasks/selftest/holdctrl"
fi
READY_SLEEP=7        # tail sleep after a non speech take, as run_take.sh
PASTE_TIMEOUT=75     # seconds to wait for a phrase to show up in TextEdit
ASR_TIMEOUT=90       # seconds to wait for committed segments on long takes
SPEECH_SLEEP=25      # extra tail sleep for the 5 minute takes
PHRASE="The quick brown fox jumps over the lazy dog."
FRAGMENT="quick brown"
# Roughly 24 words, about 25 to 30 seconds spoken at the default rate.
LONG_PARA="The morning light came slowly through the eastern window while the
city outside was still waking up and the first trains began to run along the
river crossing the bridge toward the old stone tower near the harbour wall."

if [ -z "$OUT" ]; then
  OUT="$DIR/soak-$(date +%Y%m%d-%H%M%S)"
fi

# Shared per-take state (reset at the top of every iteration).
front="" seg=0 audio=0 tail=0 paste_sent=0 rec_before=0
pid_before_run=0 rss_before=0 base_commits=0 pasted=0 hold=0

# Deterministic shape plan. Every 50th take is a 5 minute speech take.
plan_takes() {
  python3 - "$TAKES" "$SEED" <<'PY'
import random
import sys

n = int(sys.argv[1])
seed = int(sys.argv[2])
rng = random.Random(seed)
base = ["speech2", "tap", "speech2", "silence", "speech2", "spam",
        "speech2", "app_switch", "speech2", "silence"]
out = []
for i in range(1, n + 1):
    if n >= 50 and i % 50 == 0:
        out.append("speech300")
    else:
        out.append(rng.choice(base))
sys.stdout.write("\n".join(out) + "\n")
PY
}

hold_for_shape() {
  case "$1" in
    tap) echo 0.2 ;;
    silence) echo 2 ;;
    speech2) echo 3.2 ;;
    app_switch) echo 3.2 ;;
    speech30) echo 32 ;;
    speech300) echo 320 ;;
    spam) echo 0 ;;   # spam is a burst of 5 presses, handled inline
    *) echo 3 ;;
  esac
}

# Re check the frontmost app immediately before every hotkey press.
# Aborts the whole run on anything other than TextEdit, without pressing.
press_gate_or_abort() {
  local f
  f=$(frontmost_app)
  if [ "$f" != "TextEdit" ]; then
    echo "ABORT: frontmost app is '$f', not TextEdit; not pressing any key." >&2
    echo "aborted at $(date '+%Y-%m-%dT%H:%M:%S%z') frontmost=$f" > "$OUT/aborted.txt"
    exit 3
  fi
  front="$f"
}

# Speak roughly $1 seconds of text in short chunks while the key stays held.
say_chunks() {
  local secs="$1"
  local chunks i
  chunks=$(( secs / 27 ))
  [ "$chunks" -lt 1 ] && chunks=1
  for i in $(seq 1 "$chunks"); do
    say -v Samantha "$LONG_PARA"
  done
}

# Echo 1 once the fragment count grows past pasted_check_start, else 0 after
# $2 seconds. Never prints any dictated text.
wait_for_paste() {
  local frag="$1" timeout="$2"
  local deadline n
  deadline=$(( SECONDS + timeout ))
  while [ "$SECONDS" -lt "$deadline" ]; do
    n=$(count_fragment "$frag")
    if [ "${n:-0}" -gt 0 ]; then
      echo 1
      return 0
    fi
    sleep 3
  done
  echo 0
}

# Echo 1 once the latest trace reports more committed segments than the
# take started with, else 0 after $2 seconds. Reads counts only.
wait_for_commit() {
  local timeout="$1" before="$2"
  local deadline seg
  deadline=$(( SECONDS + timeout ))
  while [ "$SECONDS" -lt "$deadline" ]; do
    seg=$(baseline_commits)
    if [ "${seg:-0}" -gt "$before" ]; then
      echo 1
      return 0
    fi
    sleep 5
  done
  echo 0
}

# Committed segments on the latest trace row, counts only, 0 on any problem.
baseline_commits() {
  local tf seg
  tf=$(latest_trace || true)
  seg=0
  if [ -n "$tf" ]; then
    seg=$(python3 -I -c '
import json, sys
rows = []
try:
    rows = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
except Exception:
    rows = []
print(rows[-1].get("committed_segments", 0) if rows else 0)
' "$tf" 2>/dev/null || true)
  fi
  printf '%s' "${seg:-0}"
}

# Numeric fields off the last trace row, pipe separated, counts only.
read_trace_fields() {
  local tf
  tf=$(latest_trace || true)
  if [ -z "$tf" ]; then
    printf '0|0|0|0|0|0\n'
    return 0
  fi
  python3 -I -c '
import json, sys
rows = []
try:
    rows = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
except Exception:
    rows = []
r = rows[-1] if rows else {}
print("|".join(str(r.get(k) or 0) for k in
    ("committed_segments", "audio_ms", "tail_audio_ms", "t_asr_start",
     "t_asr_end", "t_paste_sent")))
' "$tf" 2>/dev/null || printf '0|0|0|0|0|0\n'
}

finish_take() {
  # $1 index, $2 shape, $3 start time. All other values come from the shared
  # globals set by the take loop above.
  local idx="$1" shape="$2" start_ts="$3"
  local end_ts trace rec_after rec_delta
  local pid_after rss_after pass=1 reason="" pills
  end_ts=$(date '+%Y-%m-%dT%H:%M:%S%z')

  pid_after=$(app_pid || true)
  rss_after=$(rss_kb "${pid_after:-0}" || true)
  rec_after=$(recovery_count)
  rec_delta=$(( rec_after - rec_before ))
  trace=$(latest_trace || true)

  # The Teletype process must keep the same identity for the whole take.
  if [ -z "$pid_after" ]; then
    pass=0; reason="$reason crash:no_app_process"
  elif [ "$pid_before_run" != "0" ] && [ "$pid_after" != "$pid_before_run" ]; then
    pass=0; reason="$reason crash:pid_changed"
  fi

  # Paste expectations per shape. Exact phrase for short speech, non empty
  # transcript for the long takes.
  case "$shape" in
    speech2|app_switch|speech30)
      if [ "$pasted" != "1" ]; then pass=0; reason="$reason paste_missing"; fi
      ;;
    speech300)
      if [ "$seg" -le "$base_commits" ]; then
        pass=0; reason="$reason transcript_empty"
      fi
      ;;
  esac

  # The recovery spool must be unchanged by a clean take.
  if [ "$rec_delta" -ne 0 ]; then
    pass=0; reason="$reason recovery_delta:$rec_delta"
  fi

  # Best effort stuck pill check on overlay windows.
  pills=$(pill_windows | grep -ic 'pill' 2>/dev/null || true)
  if [ "${pills:-0}" -gt 0 ]; then
    pass=0; reason="$reason pill_window"
  fi

  if [ "$pass" = "0" ]; then
    reason="$(printf '%s' "$reason" | sed 's/^ //')"
  else
    reason="ok"
  fi

  python3 - "$OUT" "$idx" "$shape" "$start_ts" "$end_ts" "$front" "$trace" \
    "$seg" "$audio" "$tail" "$paste_sent" "$rec_before" "$rec_after" \
    "$rec_delta" "$pid_before_run" "$pid_after" "$rss_before" "$rss_after" \
    "$pasted" "$pass" "$reason" <<'PY'
import json
import sys

out, idx, shape, start, end, front, trace, seg, audio, tail, paste_sent, \
    rec_before, rec_after, rec_delta, pid_before, pid_after, rss_before, \
    rss_after, pasted, ok, reason = sys.argv[1:]
try:
    paste_val = float(paste_sent) if paste_sent else 0.0
except ValueError:
    paste_val = 0.0
row = {
    "index": int(idx), "shape": shape, "start": start, "end": end,
    "frontmost": front, "trace_file": trace,
    "committed_segments": int(float(seg or 0)),
    "audio_ms": int(float(audio or 0)),
    "tail_audio_ms": int(float(tail or 0)),
    "t_paste_sent": paste_val,
    "recovery_before": int(rec_before), "recovery_after": int(rec_after),
    "recovery_delta": int(rec_delta),
    "pid_before": pid_before, "pid_after": pid_after,
    "rss_before_kb": int(rss_before or 0), "rss_after_kb": int(rss_after or 0),
    "pasted": pasted == "1", "pass": ok == "1", "reason": reason,
}
with open(out + "/takes.jsonl", "a") as f:
    f.write(json.dumps(row) + "\n")
print("take {idx:>4} {shape:<11} {verdict} seg={seg} rec_delta={rec}"
      " rss={rss} {reason}".format(
          idx=row["index"], shape=row["shape"],
          verdict="PASS" if row["pass"] else "FAIL",
          seg=row["committed_segments"], rec=row["recovery_delta"],
          rss=row["rss_after_kb"], reason=row["reason"]))
PY
}

dry_run() {
  echo "== soak dry run, takes=$TAKES seed=$SEED out=$OUT, no keys pressed"
  local ok=1 n=0
  if command -v ocu >/dev/null 2>&1; then echo "ok: ocu found at $(command -v ocu)"
  else echo "error: ocu not on PATH"; ok=0; fi
  if command -v say >/dev/null 2>&1; then echo "ok: say found"
  else echo "error: say not on PATH"; ok=0; fi
  if [ -x "$HOLDCTRL" ]; then echo "ok: holdctrl at $HOLDCTRL"
  else echo "error: holdctrl not executable at $HOLDCTRL (copy it next to soak.sh or set TELETYPE_HOLDCTRL)"; ok=0; fi
  if osascript -e 'application "TextEdit" exists' 2>/dev/null | grep -q true; then
    echo "ok: TextEdit installed"
  else
    echo "error: TextEdit not found"; ok=0
  fi
  if app_pid >/dev/null 2>&1; then
    echo "ok: Teletype app running (pid $(app_pid))"
  else
    echo "error: Teletype app not running (checked TELETYPE_APP_PID, Teletype.app, teletype-desktop)"
    echo "hint: launch the app, or export TELETYPE_APP_PID=<pid> if its process name differs"
    ok=0
  fi
  if [ -d "$TRACES" ]; then echo "ok: traces dir $TRACES"
  else echo "error: traces dir missing: $TRACES"; ok=0; fi
  n=$(recovery_count)
  if [ "$n" -gt 0 ]; then
    echo "ok: recovery spool readable, files=$n"
  else
    echo "ok: recovery spool readable, files=0"
  fi

  echo "== plan (first 20 of $TAKES takes)"
  plan_takes | head -20 | awk '{printf "%4d %s\n", NR, $1}'
  echo "== shape mix over all $TAKES takes"
  plan_takes | sort | uniq -c | sort -rn | awk '{printf "%6d %s\n", $1, $2}'
  if [ "$ok" != "1" ]; then
    echo "DRY_RUN_FAILED: fix the errors above before the real run" >&2
    exit 1
  fi
  echo "== environment valid. Real run:"
  echo "   bash tasks/selftest/soak.sh --takes $TAKES --seed $SEED"
}

summary() {
  python3 - "$OUT" "$TAKES" "$SEED" "$RESUME" <<'PY'
import collections
import json
import os
import sys

out, takes_total, seed, resume = sys.argv[1:]
path = os.path.join(out, "takes.jsonl")
rows = []
if os.path.exists(path):
    with open(path) as f:
        rows = [json.loads(l) for l in f if l.strip()]
completed = [r for r in rows if r.get("pass")]
failed = [r for r in rows if not r.get("pass")]
by_shape = collections.defaultdict(collections.Counter)
for r in rows:
    by_shape[r["shape"]]["pass" if r["pass"] else "fail"] += 1
first_fail = next((r["index"] for r in rows if not r["pass"]), None)
crashes = sum(1 for r in rows if "crash" in r.get("reason", ""))
stuck = sum(1 for r in rows if "pill" in r.get("reason", ""))
rec_bad = sum(1 for r in rows if r["recovery_delta"] != 0)
paste_fail = sum(1 for r in rows if "paste_missing" in r.get("reason", ""))
empty_fail = sum(1 for r in rows
                 if "transcript_empty" in r.get("reason", ""))
rss_at_10 = next((r["rss_after_kb"] for r in rows if r["index"] == 10), None)
rss_last = rows[-1]["rss_after_kb"] if rows else None
growth = (rss_last - rss_at_10) if (rss_at_10 is not None and rss_last) else None
under_100 = None if growth is None else growth < 102400
overall = (crashes == 0 and stuck == 0 and rec_bad == 0 and paste_fail == 0
           and empty_fail == 0 and first_fail is None
           and not (growth and growth >= 102400))
summary = {
    "takes_planned": int(takes_total), "seed": int(seed),
    "resumed": bool(int(resume)),
    "takes_recorded": len(rows), "passes": len(completed),
    "fails": len(failed), "first_fail_index": first_fail,
    "crashes": crashes, "stuck_pill_flags": stuck,
    "recovery_delta_fails": rec_bad, "paste_failures": paste_fail,
    "transcript_empty_failures": empty_fail,
    "rss_take10_kb": rss_at_10, "rss_last_kb": rss_last,
    "memory_growth_kb": growth, "memory_growth_under_100mb": under_100,
    "counts_by_shape": {s: dict(c) for s, c in sorted(by_shape.items())},
    "overall_pass": overall,
}
with open(os.path.join(out, "summary.json"), "w") as f:
    json.dump(summary, f, indent=2)
with open(os.path.join(out, "summary.md"), "w") as f:
    f.write("# soak summary\n\n")
    f.write("- takes planned: %s, recorded: %d\n"
            % (summary["takes_planned"], len(rows)))
    f.write("- pass: %d, fail: %d\n" % (len(completed), len(failed)))
    f.write("- first failing take: %s\n" % first_fail)
    f.write("- crashes: %d, stuck pill flags: %d, recovery delta fails: %d\n"
            % (crashes, stuck, rec_bad))
    f.write("- paste failures: %d, transcript empty failures: %d\n"
            % (paste_fail, empty_fail))
    if growth is not None:
        f.write("- memory take 10 to last: %d KB (%s 100 MB)\n"
                % (growth, "under" if under_100 else "OVER"))
    if stuck:
        f.write("- stuck pill flags need a live check: see README step 3\n")
    f.write("\n| shape | pass | fail |\n|---|---|---|\n")
    for s, c in sorted(by_shape.items()):
        f.write("| %s | %d | %d |\n" % (s, c["pass"], c["fail"]))
    f.write("\nFail reasons are short tags, see README for what each one\n"
            "means and how to recover.\n")
print("overall_pass=%s takes_recorded=%d first_fail_index=%s growth_kb=%s"
      % (overall, len(rows), first_fail, growth))
PY
}

if [ "$DRY_RUN" = "1" ]; then
  dry_run
  exit 0
fi

# Environment guard before anything is pressed or opened.
if ! command -v ocu >/dev/null 2>&1; then echo "ocu not on PATH" >&2; exit 1; fi
if [ ! -x "$HOLDCTRL" ]; then
  echo "holdctrl not executable at $HOLDCTRL; copy it next to soak.sh or set TELETYPE_HOLDCTRL" >&2
  exit 1
fi

mkdir -p "$OUT"
chmod 700 "$OUT"

START_AT=1
if [ "$RESUME" = "1" ] && [ -f "$OUT/takes.jsonl" ]; then
  START_AT=$(python3 -I -c '
import json, sys
rows = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
print(max((r["index"] for r in rows), default=0) + 1)
' "$OUT/takes.jsonl" 2>/dev/null || echo 1)
  echo "resume: continuing from take index $START_AT"
fi

echo "== soak run: out=$OUT takes=$TAKES seed=$SEED start_at=$START_AT"
if app_pid >/dev/null 2>&1; then
  echo "pid_start=$(app_pid)"
else
  echo "error: Teletype app not running; refusing to start" >&2
  exit 1
fi

i=0
while IFS= read -r shape; do
  i=$((i + 1))
  if [ "$i" -lt "$START_AT" ]; then continue; fi
  start_ts=$(date '+%Y-%m-%dT%H:%M:%S%z')
  pasted=0
  pid_before_run=$(app_pid || echo 0)
  rss_before=$(rss_kb "$pid_before_run" || echo 0)
  rec_before=$(recovery_count)
  base_commits=$(baseline_commits)

  open_textedit
  front=$(frontmost_app)
  press_gate_or_abort

  hold=$(hold_for_shape "$shape")
  case "$shape" in
    spam)
      # 5 discrete presses inside about 2 seconds, gate checked each time.
      for _ in 1 2 3 4 5; do
        press_gate_or_abort
        "$HOLDCTRL" 0.18 >/dev/null 2>&1 &
        sleep 0.28
      done
      wait
      ;;
    speech2|app_switch|speech30)
      "$HOLDCTRL" "$hold" >/dev/null 2>&1 &
      HOLD_PID=$!
      sleep 0.8
      if [ "$shape" = "speech30" ]; then
        say -v Samantha "$LONG_PARA"
      else
        say -v Samantha "$PHRASE"
      fi
      wait "$HOLD_PID"
      if [ "$shape" = "app_switch" ]; then
        open -a Finder
        sleep 1
        open -a TextEdit
        press_gate_or_abort
      fi
      sleep "$READY_SLEEP"
      pasted=$(wait_for_paste "$FRAGMENT" "$PASTE_TIMEOUT")
      ;;
    speech300)
      "$HOLDCTRL" "$hold" >/dev/null 2>&1 &
      HOLD_PID=$!
      sleep 0.8
      say_chunks 300
      wait "$HOLD_PID"
      sleep "$SPEECH_SLEEP"
      got=$(wait_for_commit "$ASR_TIMEOUT" "$base_commits")
      if [ "$got" = "1" ]; then pasted=1; fi
      ;;
    *)
      "$HOLDCTRL" "$hold" >/dev/null 2>&1 &
      HOLD_PID=$!
      wait "$HOLD_PID"
      ;;
  esac

  # Trace numbers for the record. Counts only, never transcript text.
  fields=$(read_trace_fields)
  seg="${fields%%|*}"
  rest="${fields#*|}"
  audio="${rest%%|*}"
  rest="${rest#*|}"
  tail="${rest%%|*}"
  rest="${rest#*|}"
  paste_sent="${rest##*|}"

  finish_take "$i" "$shape" "$start_ts"
done < <(plan_takes)

summary
