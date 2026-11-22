#!/usr/bin/env bash
# Release gate: the main thread must stay responsive across a
# background/foreground cycle.
#
# v0.2.21 shipped a main-thread hang: the Fn tap teardown joined a thread whose
# run loop had not started yet, and the app froze with no window repaint and no
# Dock response (learning L095). Nothing in the product said so — the freeze was
# only visible as a user report — and nothing in QA could see it either, because
# a hung main thread and an idle one block in the same place when sampled.
#
# So the app carries a watchdog (`crates/teletype-desktop/src/main_thread_watchdog.rs`)
# that queues a no-op closure on the main thread every few seconds and logs a
# `teletype-watchdog: main thread unresponsive` line to stderr when the closure
# is not served. This script drives the real bundle and reads that log:
#
#   1. launch the built .app with its stderr captured
#   2. wait for the watchdog's first answered probe (readiness)
#   3. cycle to the background (`open -a Finder`) and back to the foreground
#      (`open -a <bundle>`) --cycles times. Plain `open`, no Apple Events, so
#      no Accessibility/Automation grant is needed and nothing has to be
#      installed on the machine.
#   4. fail when the log shows an unresponsive line, when the app died, or when
#      the watchdog stopped answering probes after the cycle (proven by its
#      heartbeat lines, which the gate turns on through the app's env seam).
#
# The probe cadence is sped up from the 5 s production default to 300 ms and
# the grace period to 4 s, so the whole gate takes ~20 s. A main thread blocked
# for 4 s is already a hang to a person typing.
#
# Usage: scripts/check-main-thread-responsive.sh [--dry] [--app PATH] [--cycles N] [--strict]
#   --dry        print the plan and exit 0 without launching anything
#   --app PATH   .app bundle to drive (default: the newest built bundle)
#   --cycles N   background/foreground cycles to run (default 3)
#   --strict     fail instead of skipping when the run cannot be observed
#
# Exit 0: the main thread stayed responsive, or the gate skipped (not macOS, no
# built bundle, app never reached its first probe). Exit 1: a real failure.
# A gate that cannot observe must not block a release, but it must say so — and
# --strict turns every skip into a failure for a pipeline that has to prove it.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

PROBE_MS=300
TIMEOUT_MS=4000
HEARTBEAT_PROBES=4
STARTUP_TIMEOUT_S=90
SETTLE_S=6

DRY=0
STRICT=0
CYCLES=3
APP=""

while [ $# -gt 0 ]; do
  case "$1" in
    --dry) DRY=1 ;;
    --strict) STRICT=1 ;;
    --app)
      APP="${2:-}"
      shift
      ;;
    --cycles)
      CYCLES="${2:-}"
      shift
      ;;
    -h|--help)
      sed -n '2,45p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
  shift
done

skip() {
  echo "SKIP: $*"
  if [ "$STRICT" = "1" ]; then
    echo "RESULT: FAIL (--strict: a skipped gate is a failed gate)"
    exit 1
  fi
  echo "RESULT: SKIP"
  exit 0
}

fail() {
  echo "RESULT: FAIL — $*"
  exit 1
}

find_bundle() {
  local candidate
  if [ -n "${TELETYPE_APP_BUNDLE:-}" ]; then
    echo "$TELETYPE_APP_BUNDLE"
    return 0
  fi
  for candidate in \
    target/release/bundle/macos/Teletype.app \
    target/debug/bundle/macos/Teletype.app; do
    if [ -d "$candidate" ]; then
      echo "$candidate"
      return 0
    fi
  done
  return 1
}

if [ "$(uname -s)" != "Darwin" ]; then
  skip "not macOS: this gate drives a real .app through LaunchServices"
fi

if [ -z "$APP" ]; then
  APP="$(find_bundle)" || APP=""
fi

if [ "$DRY" = "1" ]; then
  echo "dry run: would drive ${APP:-the newest built Teletype.app}"
  echo "  probes:    every ${PROBE_MS} ms, unresponsive after ${TIMEOUT_MS} ms"
  echo "  heartbeat: every ${HEARTBEAT_PROBES} answered probes"
  echo "  cycles:    $CYCLES x (background: open -a Finder / foreground: open -a \"${APP:-<bundle>}\")"
  echo "  run dir:   a temporary HOME so the gate never touches real app data"
  echo "  verdict:   fail on a 'teletype-watchdog: main thread unresponsive' line,"
  echo "             on the app dying, or on the heartbeat stopping after the cycle"
  echo "RESULT: PASS (dry run)"
  exit 0
fi

[ -n "$APP" ] || skip "no built Teletype.app (run \`cargo tauri build\` or the release build stage first)"
[ -d "$APP" ] || skip "app bundle not found: $APP"

EXE="$APP/Contents/MacOS/teletype"
[ -x "$EXE" ] || skip "bundle has no executable at $EXE"

# The app is single-instance (`tauri-plugin-single-instance`): launching it
# while another copy runs makes the new process hand its arguments over and
# exit at once, which would look exactly like "the app died" to this gate. The
# plugin keys on the bundle identifier, so an already-running Teletype with the
# same identifier blocks the run.
app_id() {
  plutil -extract CFBundleIdentifier raw "$1/Contents/Info.plist" 2>/dev/null
}

BUNDLE_ID="$(app_id "$APP")"
if [ -n "$BUNDLE_ID" ]; then
  for pid in $(pgrep -x teletype 2>/dev/null); do
    running_exe="$(ps -o comm= -p "$pid" 2>/dev/null)"
    case "$running_exe" in
      */Contents/MacOS/*) ;;
      *) continue ;;
    esac
    running_app="${running_exe%%/Contents/MacOS/*}"
    [ "$(app_id "$running_app")" = "$BUNDLE_ID" ] || continue
    skip "Teletype ($BUNDLE_ID) is already running as pid $pid; quit it and re-run — a second launch exits immediately (single-instance plugin), so there would be nothing to observe"
  done
fi

echo "gate: main thread responsive across background/foreground cycles"
echo "app:  $APP"

RUN_DIR="$(mktemp -d "${TMPDIR:-/tmp}/teletype-main-thread-gate.XXXXXX")"
LOG="$RUN_DIR/app.log"
mkdir -p "$RUN_DIR/home"
PID=""

cleanup() {
  if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
    kill "$PID" 2>/dev/null
    # Bounded wait: a terminating app that will not exit is itself a finding,
    # but it must not hang the gate.
    local waited=0
    while kill -0 "$PID" 2>/dev/null && [ "$waited" -lt 20 ]; do
      sleep 0.1
      waited=$((waited + 1))
    done
    kill -9 "$PID" 2>/dev/null
  fi
  if [ "${KEEP_GATE_LOG:-0}" = "1" ]; then
    echo "log kept at $LOG"
  else
    rm -rf "$RUN_DIR"
  fi
}
trap cleanup EXIT

# A fresh HOME keeps the gate away from the user's dictionary, models and
# settings: the gate must not change what the app remembers, and onboarding
# state from a real install must not change what the gate measures.
HOME="$RUN_DIR/home" \
  TELETYPE_MAIN_THREAD_PROBE_MS="$PROBE_MS" \
  TELETYPE_MAIN_THREAD_TIMEOUT_MS="$TIMEOUT_MS" \
  TELETYPE_MAIN_THREAD_HEARTBEAT_PROBES="$HEARTBEAT_PROBES" \
  "$EXE" >"$LOG" 2>&1 &
PID=$!
# Without this, killing the app from the trap prints a "Terminated: 15" job
# notification into the gate's output, which reads like a failure.
disown "$PID" 2>/dev/null

echo "pid:  $PID (log: $LOG)"

# --- Readiness: the main thread answered at least one probe -----------------
ready=0
waited=0
while [ "$waited" -lt $((STARTUP_TIMEOUT_S * 10)) ]; do
  if grep -q "main-thread watchdog: first probe answered" "$LOG" 2>/dev/null; then
    ready=1
    break
  fi
  if ! kill -0 "$PID" 2>/dev/null; then
    break
  fi
  sleep 0.1
  waited=$((waited + 1))
done

if [ "$ready" = "0" ]; then
  echo "--- app log (last 40 lines) ---"
  tail -40 "$LOG" 2>/dev/null
  echo "------------------------------"
  if kill -0 "$PID" 2>/dev/null; then
    fail "the app never answered a main-thread probe in ${STARTUP_TIMEOUT_S}s (watchdog did not start, or the app has no event loop yet)"
  fi
  skip "the app exited before its first probe, so there was nothing to observe (another copy already running, or a launch failure: see the log above)"
fi

echo "ready: watchdog is answering probes"

# --- Background/foreground cycles ------------------------------------------
heartbeats() {
  grep -c "probes answered, main thread responsive" "$LOG" 2>/dev/null || true
}

cycle=1
while [ "$cycle" -le "$CYCLES" ]; do
  before="$(heartbeats)"
  open -a Finder >/dev/null 2>&1 || echo "  warning: could not activate Finder for the background step"
  sleep 1.5
  if ! kill -0 "$PID" 2>/dev/null; then
    fail "the app exited during background step $cycle"
  fi
  open -a "$APP" >/dev/null 2>&1 || echo "  warning: could not re-activate $APP for the foreground step"
  sleep 2
  if ! kill -0 "$PID" 2>/dev/null; then
    fail "the app exited during foreground step $cycle"
  fi
  echo "  cycle $cycle/$CYCLES: app alive (heartbeats before: $before)"
  cycle=$((cycle + 1))
done

# --- Verdict ---------------------------------------------------------------
# The last cycle's foreground step has just run; give the watchdog several
# probe periods to either answer (heartbeat) or go missing.
heartbeats_before_settle="$(heartbeats)"
sleep "$SETTLE_S"
heartbeats_after_settle="$(heartbeats)"

unresponsive="$(grep -c "teletype-watchdog: main thread unresponsive" "$LOG" 2>/dev/null || true)"
if [ "${unresponsive:-0}" -gt 0 ]; then
  echo "--- watchdog reports ---"
  grep "teletype-watchdog: main thread unresponsive" "$LOG" | head -5
  echo "--- app log (last 40 lines) ---"
  tail -40 "$LOG"
  fail "the main thread stopped answering probes after the background/foreground cycle"
fi

if ! kill -0 "$PID" 2>/dev/null; then
  echo "--- app log (last 40 lines) ---"
  tail -40 "$LOG"
  fail "the app died during the cycle"
fi

if [ "${heartbeats_after_settle:-0}" -le "${heartbeats_before_settle:-0}" ]; then
  echo "--- app log (last 40 lines) ---"
  tail -40 "$LOG"
  fail "the watchdog stopped answering probes after the cycle (no heartbeat in ${SETTLE_S}s): the main thread is not reaching its event loop"
fi

echo "verdict: no unresponsiveness, watchdog still answering (heartbeats $heartbeats_before_settle -> $heartbeats_after_settle)"
echo "RESULT: PASS"
exit 0
