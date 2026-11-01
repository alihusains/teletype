#!/usr/bin/env bash
# End-to-end dictation self-test through the REAL microphone path.
# 1. Opens a fresh TextEdit document and makes it frontmost (so the paste
#    can never land in the user's terminal).
# 2. Holds Control (the configured hotkey) via the HID event stream while
#    macOS `say` speaks a fixed synthetic sentence through the speakers.
# 3. Reads back what Teletype pasted into TextEdit (via Open Computer Use)
#    and the latest take trace's numeric fields.
# Usage: run_take.sh [seconds_to_hold] [phrase]
set -u
DIR="$(cd "$(dirname "$0")" && pwd)"
HOLD="${1:-6}"
PHRASE="${2:-The quick brown fox jumps over the lazy dog.}"
TRACES="$HOME/Library/Application Support/com.teletype.app/traces"

osascript -e 'tell application "TextEdit" to make new document' \
          >/dev/null
open -a TextEdit
sleep 1.5
front=$(osascript -e 'tell application "System Events" to get name of first process whose frontmost is true')
if [ "$front" != "TextEdit" ]; then
  echo "ABORT: frontmost app is '$front', not TextEdit; not pressing any key."
  exit 2
fi

# holdctrl lives in the main checkout; a local copy is optional. Explicit
# TELETYPE_HOLDCTRL wins, then the local copy, then the main checkout.
HOLDCTRL="${TELETYPE_HOLDCTRL:-$DIR/holdctrl}"
if [ -z "${TELETYPE_HOLDCTRL:-}" ] && [ ! -e "$HOLDCTRL" ]; then
  HOLDCTRL="/Users/a.sorathiya/Documents/Ali/teletype/tasks/selftest/holdctrl"
fi
if [ ! -x "$HOLDCTRL" ]; then
  echo "holdctrl not executable at $HOLDCTRL; copy it next to run_take.sh or set TELETYPE_HOLDCTRL" >&2
  exit 1
fi
"$HOLDCTRL" "$HOLD" &
HOLD_PID=$!
sleep 0.8
say -v Samantha "$PHRASE"
wait "$HOLD_PID"
sleep 7

echo "== TextEdit text (expected: the phrase)"
ocu call get_app_state --args '{"app":"TextEdit","text_limit":1000}' 2>/dev/null \
  | grep -oiE 'quick brown[^"\\]{0,60}|fox jumps[^"\\]{0,40}' | head -2

echo "== latest take trace"
latest="$TRACES/$(ls -t "$TRACES" | head -1)"
python3 -I - "$latest" <<'PY'
import json, sys
rows = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
r = rows[-1]
keep = ("session", "engine", "audio_ms", "committed_segments", "tail_audio_ms",
        "t_asr_start", "t_asr_end", "t_paste_sent", "polish_skipped_reason")
print({k: r.get(k) for k in keep}, "capture_ms=", (r.get("stage_ms") or {}).get("capture"))
PY
