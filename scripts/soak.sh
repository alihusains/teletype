#!/usr/bin/env bash
# Teletype soak test (plan §9.1 soak row).
#
# Repeatedly triggers one dictation and samples the target process's RSS and
# FD count every 60 s, then compares first vs last sample against growth
# limits. Writes JSONL samples + a final JSON summary.
#
# Usage:
#   scripts/soak.sh --trigger-cmd '...' [--duration-s 28800] [--interval-s 20]
#                   [--rss-growth-limit-mb 50] [--fd-growth-limit 16]
#                   [--process <name-or-pid>] [--max-trigger-failures N]
#                   [--out DIR]
#                   [--selfcheck]
#
#   --trigger-cmd  command that triggers one dictation (required unless
#                  --selfcheck). If absent, exit 2 with instructions.
#   --process      process name (pgrep -f) or numeric PID to observe. If the
#                  target cannot be resolved at start, the instrument is
#                  invalid: verdict INSTRUMENT_INVALID, exit 3. If omitted
#                  (and not --selfcheck), the run is blind: RSS/FD are never
#                  sampled, verdict INSTRUMENT_INVALID, exit 3.
#   --max-trigger-failures N
#                  maximum number of failed trigger invocations allowed
#                  (default 0). Exceeding it records an unobserved entry and
#                  forces INSTRUMENT_INVALID.
#   --selfcheck    3 iterations with `sleep 1` as the trigger; must pass on
#                  any dev machine. No process sampling.
#
# Exit codes:
#   0  verdict PASS: instrument valid, no growth limit exceeded
#   1  verdict FAIL: instrument valid, a growth limit was exceeded
#   2  bad usage (wrong flags / missing --trigger-cmd)
#   3  verdict INSTRUMENT_INVALID: the run could not observe what it claims
#      to measure (no --process, unresolvable target, missed samples, or
#      trigger failures past the limit). A blind run never prints PASS.
set -euo pipefail

DURATION_S=28800
INTERVAL_S=20
RSS_LIMIT_MB=50
FD_LIMIT=16
TRIGGER_CMD=""
PROCESS=""
MAX_TRIGGER_FAILURES=0
OUT_DIR=""
SELFCHECK=0

while [ $# -gt 0 ]; do
  case "$1" in
    --duration-s) DURATION_S="$2"; shift 2 ;;
    --interval-s) INTERVAL_S="$2"; shift 2 ;;
    --rss-growth-limit-mb) RSS_LIMIT_MB="$2"; shift 2 ;;
    --fd-growth-limit) FD_LIMIT="$2"; shift 2 ;;
    --trigger-cmd) TRIGGER_CMD="$2"; shift 2 ;;
    --process) PROCESS="$2"; shift 2 ;;
    --max-trigger-failures) MAX_TRIGGER_FAILURES="$2"; shift 2 ;;
    --out) OUT_DIR="$2"; shift 2 ;;
    --selfcheck) SELFCHECK=1; shift ;;
    -h|--help)
      sed -n '2,35p' "$0" | sed 's/^# \{0,1\}//'
      exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

if [ "$SELFCHECK" = "1" ]; then
  # Selfcheck validates script mechanics (arg parsing, trigger loop), not
  # process observation: it never samples and always exits 0 when it runs.
  echo "soak selfcheck: 3 iterations, trigger='sleep 1'"
  for i in 1 2 3; do
    echo "  iteration $i: sleep 1"
    sleep 1
  done
  echo "SELF CHECK: PASS"
  exit 0
fi

if [ -z "$TRIGGER_CMD" ]; then
  cat >&2 <<'EOF'
soak: --trigger-cmd is required (a shell command that triggers one
dictation of the app under test). Example:

  scripts/soak.sh --trigger-cmd 'osascript -e "tell application \"System Events\" to keystroke space using {control down, command down}"' \
      --process Teletype --duration-s 3600

or run `scripts/soak.sh --selfcheck` to verify this script itself.
EOF
  exit 2
fi

if [ -z "$OUT_DIR" ]; then
  OUT_DIR="soak-out-$(date +%Y%m%d-%H%M%S)"
fi
mkdir -p "$OUT_DIR"
JSONL="$OUT_DIR/samples.jsonl"
SUMMARY="$OUT_DIR/summary.json"
: > "$JSONL"

# --- instrument-validity state -------------------------------------------------
# UNOBSERVED collects machine-readable reasons this run could not observe
# what it claims to measure. Non-empty => verdict INSTRUMENT_INVALID, exit 3.
UNOBSERVED=()
add_unobserved() {
  UNOBSERVED+=("$1")
}

TRIGGERS_RUN=0
TRIGGER_FAILURES=0
SAMPLES=0
OBSERVED_SAMPLES=0
SAMPLE_MISS=0
FIRST_MISS_SAMPLE=0

# --- resolve target process --------------------------------------------------
TARGET_PID=""
resolve_target() {
  case "$PROCESS" in
    ''|*[!0-9]*)  # name (or empty)
      if [ -n "$PROCESS" ]; then
        TARGET_PID="$(pgrep -f "$PROCESS" | head -1 || true)"
      fi
      ;;
    *) TARGET_PID="$PROCESS" ;;
  esac
}

if [ -n "$PROCESS" ]; then
  resolve_target
  if [ -z "$TARGET_PID" ]; then
    echo "soak: cannot find process matching '$PROCESS' (pgrep -f); instrument invalid" >&2
    add_unobserved "process '$PROCESS' not found at start (pgrep -f)"
  elif ! kill -0 "$TARGET_PID" 2>/dev/null; then
    echo "soak: PID $TARGET_PID ($PROCESS) is not alive at start; instrument invalid" >&2
    add_unobserved "process PID $TARGET_PID not alive at start (kill -0)"
    TARGET_PID=""
  else
    echo "soak: target PID $TARGET_PID ($PROCESS)"
  fi
else
  echo "soak: no --process given; RSS/FD will never be sampled; instrument invalid" >&2
  add_unobserved "no --process given; RSS/FD never sampled"
fi

# --- sample() -----------------------------------------------------------------
# Every sample must observe a live process. If it cannot (process gone,
# rss empty/0, fd=0/-1: FD count 0 means lsof could not observe the target,
# not that the target has no descriptors), record a single unobserved entry
# with a running miss count and force INSTRUMENT_INVALID. Never silently emit
# an unobserved sample as a passing measurement.
sample() {
  local now rss fd
  now="$(date +%s)"
  SAMPLES=$((SAMPLES + 1))
  if [ -n "$TARGET_PID" ] && kill -0 "$TARGET_PID" 2>/dev/null; then
    rss="$(ps -o rss= -p "$TARGET_PID" 2>/dev/null | tr -d ' ' || echo 0)"
    fd="$(lsof -p "$TARGET_PID" 2>/dev/null | wc -l | tr -d ' ')"
  else
    rss=0; fd=-1
  fi
  LAST_RSS="$rss"
  LAST_FD="$fd"
  if [ -z "$rss" ] || [ "$rss" = "0" ] || [ "$fd" = "0" ] || [ "$fd" = "-1" ]; then
    SAMPLE_MISS=$((SAMPLE_MISS + 1))
    if [ "$FIRST_MISS_SAMPLE" = "0" ]; then
      FIRST_MISS_SAMPLE="$SAMPLES"
      add_unobserved "process not observed at sample ${FIRST_MISS_SAMPLE} (miss count ${SAMPLE_MISS})"
    else
      # Update the existing entry with the new miss count
      for i in "${!UNOBSERVED[@]}"; do
        case "${UNOBSERVED[$i]}" in
          "process not observed at sample "*)
            UNOBSERVED[$i]="process not observed at sample ${FIRST_MISS_SAMPLE} (miss count ${SAMPLE_MISS})"
            ;;
        esac
      done
    fi
  else
    OBSERVED_SAMPLES=$((OBSERVED_SAMPLES + 1))
    if [ -z "$FIRST_RSS" ]; then
      FIRST_RSS="$rss"
      FIRST_FD="$fd"
    fi
  fi
  printf '{"ts":%s,"rss_kb":%s,"fd_count":%s}\n' "$now" "${rss:-0}" "${fd:--1}" >> "$JSONL"
}

# --- write_summary <verdict> ---------------------------------------------------
write_summary() {
  local verdict="$1"
  python3 - "$SUMMARY" "$DURATION_S" "$INTERVAL_S" "$PROCESS" "${TARGET_PID:-}" "$TRIGGER_CMD" "${FIRST_RSS:-}" "${LAST_RSS:-}" "$RSS_GROWTH_MB" "$RSS_LIMIT_MB" "${FIRST_FD:-}" "${LAST_FD:-}" "$FD_GROWTH" "$FD_LIMIT" "$JSONL" "$TRIGGERS_RUN" "$TRIGGER_FAILURES" "$MAX_TRIGGER_FAILURES" "$SAMPLES" "$OBSERVED_SAMPLES" "$verdict" ${UNOBSERVED[@]+"${UNOBSERVED[@]}"} <<'PY'
import json, sys
# args[0] = SUMMARY path, args[1..20] = 20 fields, args[21:] = unobserved
args = sys.argv[1:]
summary_path = args[0]
dur            = args[1]
interval       = args[2]
process        = args[3]
pid            = args[4]
trigger        = args[5]
first_rss      = args[6]
last_rss       = args[7]
rss_growth     = args[8]
rss_limit      = args[9]
first_fd       = args[10]
last_fd        = args[11]
fd_growth      = args[12]
fd_limit       = args[13]
samples_file   = args[14]
tr_run         = args[15]
tr_fail        = args[16]
tr_limit       = args[17]
samples        = args[18]
observed       = args[19]
verdict        = args[20]
unobserved     = args[21:]

def opt(s):
    return None if s == "" else s

def num(s):
    return None if s == "" else int(s)

summary = {
    "duration_s": int(dur),
    "interval_s": int(interval),
    "process": opt(process),
    "pid": num(pid),
    "trigger_cmd": trigger,
    "first_rss_kb": num(first_rss),
    "last_rss_kb": num(last_rss),
    "rss_growth_mb": int(rss_growth),
    "rss_limit_mb": int(rss_limit),
    "first_fd": num(first_fd),
    "last_fd": num(last_fd),
    "fd_growth": int(fd_growth),
    "fd_limit": int(fd_limit),
    "samples_file": samples_file,
    "triggers_run": int(tr_run),
    "trigger_failures": int(tr_fail),
    "max_trigger_failures": int(tr_limit),
    "samples": int(samples),
    "observed_samples": int(observed),
    "unobserved": unobserved,
    "verdict": verdict,
}
with open(summary_path, "w") as f:
    json.dump(summary, f, indent=2)
    f.write("\n")
PY
}

# --- emit_result ----------------------------------------------------------------
# Writes summary.json, prints the human RESULT line, and exits with the code
# matching the verdict precedence: INSTRUMENT_INVALID (3) > growth FAIL (1)
# > PASS (0).
emit_result() {
  local verdict="PASS"
  local rc=0
  if [ "${#UNOBSERVED[@]}" -gt 0 ]; then
    verdict="INSTRUMENT_INVALID"
    rc=3
  elif [ "$RSS_OK" = "0" ] || [ "$FD_OK" = "0" ]; then
    verdict="FAIL"
    rc=1
  fi
  write_summary "$verdict"
  echo
  echo "soak summary:"
  cat "$SUMMARY"
  if [ "$verdict" = "INSTRUMENT_INVALID" ]; then
    echo "RESULT: INSTRUMENT INVALID (${UNOBSERVED[0]})"
  elif [ "$verdict" = "FAIL" ]; then
    if [ "$RSS_OK" = "0" ]; then
      echo "RESULT: FAIL (RSS grew ${RSS_GROWTH_MB} MB > ${RSS_LIMIT_MB} MB)"
    else
      echo "RESULT: FAIL (FDs grew ${FD_GROWTH} > ${FD_LIMIT})"
    fi
  else
    echo "RESULT: PASS"
  fi
  exit "$rc"
}

# --- main loop ----------------------------------------------------------------
START_TS="$(date +%s)"
FIRST_RSS=""
FIRST_FD=""
LAST_RSS=""
LAST_FD=""

echo "soak: duration=${DURATION_S}s interval=${INTERVAL_S}s trigger='${TRIGGER_CMD}'"
while true; do
  NOW="$(date +%s)"
  ELAPSED=$((NOW - START_TS))
  if [ "$ELAPSED" -ge "$DURATION_S" ]; then
    break
  fi
  echo "[$(date +%H:%M:%S)] trigger (elapsed ${ELAPSED}s)"
  if bash -c "$TRIGGER_CMD"; then
    TRIGGERS_RUN=$((TRIGGERS_RUN + 1))
  else
    TRIGGER_FAILURES=$((TRIGGER_FAILURES + 1))
    echo "  (trigger command exited non-zero; failures=$TRIGGER_FAILURES)"
  fi

  sample

  # sleep the interval, but stop early if the duration is up
  SLEEP_S=$((DURATION_S - ELAPSED))
  if [ "$SLEEP_S" -gt "$INTERVAL_S" ]; then
    sleep "$INTERVAL_S"
  else
    sleep "$SLEEP_S"
  fi
done

sample

if [ "$TRIGGER_FAILURES" -gt "$MAX_TRIGGER_FAILURES" ]; then
  add_unobserved "trigger failures: ${TRIGGER_FAILURES} (limit ${MAX_TRIGGER_FAILURES})"
fi

# --- verdict ------------------------------------------------------------------
RSS_GROWTH_MB=0
FD_GROWTH=0
if [ -n "$FIRST_RSS" ] && [ -n "$LAST_RSS" ] && [ "$LAST_RSS" != "0" ]; then
  RSS_GROWTH_MB=$(( (LAST_RSS - FIRST_RSS) / 1024 ))
  FD_GROWTH=$((LAST_FD - FIRST_FD))
fi

RSS_OK=1
FD_OK=1
if [ -n "$TARGET_PID" ]; then
  if [ "$RSS_GROWTH_MB" -le "$RSS_LIMIT_MB" ]; then RSS_OK=1; else RSS_OK=0; fi
  if [ "$FD_GROWTH" -le "$FD_LIMIT" ]; then FD_OK=1; else FD_OK=0; fi
fi

emit_result
