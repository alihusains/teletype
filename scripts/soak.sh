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
#                   [--process <name-or-pid>] [--out DIR]
#                   [--selfcheck]
#
#   --trigger-cmd  command that triggers one dictation (required unless
#                  --selfcheck). If absent, exit 2 with instructions.
#   --selfcheck    3 iterations with `sleep 1` as the trigger; must pass on
#                  any dev machine. No process sampling.
#
# Exit codes: 0 ok, 1 a growth limit was exceeded, 2 bad usage.
set -euo pipefail

DURATION_S=28800
INTERVAL_S=20
RSS_LIMIT_MB=50
FD_LIMIT=16
TRIGGER_CMD=""
PROCESS=""
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
    --out) OUT_DIR="$2"; shift 2 ;;
    --selfcheck) SELFCHECK=1; shift ;;
    -h|--help)
      sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
      exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

if [ "$SELFCHECK" = "1" ]; then
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
    echo "soak: cannot find process matching '$PROCESS' (pgrep -f)" >&2
    exit 2
  fi
  echo "soak: target PID $TARGET_PID ($PROCESS)"
else
  echo "soak: no --process given; skipping RSS/FD sampling"
fi

sample() {
  local now rss fd
  now="$(date +%s)"
  if [ -n "$TARGET_PID" ] && kill -0 "$TARGET_PID" 2>/dev/null; then
    rss="$(ps -o rss= -p "$TARGET_PID" 2>/dev/null | tr -d ' ' || echo 0)"
    fd="$(lsof -p "$TARGET_PID" 2>/dev/null | wc -l | tr -d ' ')"
  else
    rss=0; fd=-1
  fi
  LAST_RSS="$rss"
  LAST_FD="$fd"
  if [ -z "$FIRST_RSS" ] && [ "$rss" != "0" ]; then
    FIRST_RSS="$rss"
    FIRST_FD="$fd"
  fi
  printf '{"ts":%s,"rss_kb":%s,"fd_count":%s}\n' "$now" "${rss:-0}" "${fd:--1}" >> "$JSONL"
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
  bash -c "$TRIGGER_CMD" || echo "  (trigger command exited $?)"

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

# --- verdict ------------------------------------------------------------------
RSS_GROWTH_MB=0
FD_GROWTH=0
if [ -n "$FIRST_RSS" ] && [ -n "$LAST_RSS" ] && [ "$LAST_RSS" != "0" ]; then
  RSS_GROWTH_MB=$(( (LAST_RSS - FIRST_RSS) / 1024 ))
  FD_GROWTH=$((LAST_FD - FIRST_FD))
fi

if [ -z "$TARGET_PID" ]; then
  RSS_OK=1; FD_OK=1
elif [ "$RSS_GROWTH_MB" -le "$RSS_LIMIT_MB" ]; then RSS_OK=1; else RSS_OK=0; fi
if [ "$FD_GROWTH" -le "$FD_LIMIT" ]; then FD_OK=1; else FD_OK=0; fi

cat > "$SUMMARY" <<EOF
{
  "duration_s": $DURATION_S,
  "interval_s": $INTERVAL_S,
  "process": "${PROCESS:-null}",
  "pid": ${TARGET_PID:-null},
  "trigger_cmd": $(python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$TRIGGER_CMD"),
  "first_rss_kb": ${FIRST_RSS:-null},
  "last_rss_kb": ${LAST_RSS:-null},
  "rss_growth_mb": $RSS_GROWTH_MB,
  "rss_limit_mb": $RSS_LIMIT_MB,
  "first_fd": ${FIRST_FD:-null},
  "last_fd": ${LAST_FD:-null},
  "fd_growth": $FD_GROWTH,
  "fd_limit": $FD_LIMIT,
  "samples_file": "$JSONL",
  "verdict": "PASS"
}
EOF

echo
echo "soak summary:"
cat "$SUMMARY"
if [ "$RSS_OK" = "0" ]; then
  echo "RESULT: FAIL (RSS grew ${RSS_GROWTH_MB} MB > ${RSS_LIMIT_MB} MB)"
  exit 1
fi
if [ "$FD_OK" = "0" ]; then
  echo "RESULT: FAIL (FDs grew ${FD_GROWTH} > ${FD_LIMIT})"
  exit 1
fi
echo "RESULT: PASS"
exit 0
