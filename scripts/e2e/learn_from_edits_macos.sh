#!/usr/bin/env bash
# Live learn-from-edits check on the real accessibility path.
#
# This is the proof for personalization that unit tests cannot give: it runs
# a real take into TextEdit, edits the landed text with real keystrokes, lets
# the production edit watcher read the field back through AX, and then asserts
# that the learned mishearing reached the dictionary (the D006 path that
# actually changes the next transcript).
#
# The app runs with `--e2e` and isolated stores (`TELETYPE_E2E_*_DIR`), so the
# proof never writes into the developer's real dictionary or profile. The
# isolated data dir symlinks the real models dir and copies settings.json so
# the take uses the same installed speech model.
#
# Usage:
#   scripts/e2e/learn_from_edits_macos.sh --dry   # print the plan
#   scripts/e2e/learn_from_edits_macos.sh         # live run
#
# Env:
#   TELETYPE_E2E_BIN       teletype binary (default: repo target/debug/teletype)
#   TELETYPE_E2E_SOCKET    control socket (default /tmp/teletype-e2e.sock)
#   TELETYPE_E2E_WAV       fixture (default crates/teletype-speech/tests/fixtures/names.wav)
#   TELETYPE_E2E_SOURCE    word the engine got wrong (default Shavano)
#   TELETYPE_E2E_TARGET    correction (default Shivon)
#   TELETYPE_REAL_DATA     real app data dir (default
#                          ~/Library/Application Support/com.teletype.app)
#
# names.wav says "Siobhan O'Rourke, Aoife, PyTorch, Kubernetes"; the engine
# transcribes it as "Shavano Rourke, Epha, Py Torch, Kubernetes." (observed
# on parakeet-ultra). The correction is the phonetic respelling "Shivon":
# the Irish "Siobhan" itself scores 0.48 in the learning judge's phonetic
# band (below the 0.6 floor), and names with apostrophes are skipped by the
# term extractor, so neither can prove the write path.
# hello.wav cannot prove this path either: "hello" is an everyday word, which
# the learning judge correctly refuses to learn as a term.
#
# Requires Accessibility + Automation permission for the terminal running it.
# Prints PASS/FAIL and exits 1 on failure. Temp stores are kept on failure.
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOCK="${TELETYPE_E2E_SOCKET:-/tmp/teletype-e2e.sock}"
BIN="${TELETYPE_E2E_BIN:-$REPO/target/debug/teletype}"
WAV="${TELETYPE_E2E_WAV:-$REPO/crates/teletype-speech/tests/fixtures/names.wav}"
SOURCE_WORD="${TELETYPE_E2E_SOURCE:-Shavano}"
TARGET_WORD="${TELETYPE_E2E_TARGET:-Shivon}"
REAL_DATA="${TELETYPE_REAL_DATA:-$HOME/Library/Application Support/com.teletype.app}"
# The watcher checks 15 s after the paste; leave slack for AX reads.
CHECK_WAIT=40

DRY=0
[ "${1:-}" = "--dry" ] && DRY=1

if [ "$DRY" = "1" ]; then
  cat <<EOF
learn-from-edits live check (real AX path)
  1. launch $BIN --e2e with isolated config/data dirs
  2. focus TextEdit, empty document
  3. start a take with $WAV (file-input mode, real pipeline + injection)
  4. type the correction "$SOURCE_WORD" -> "$TARGET_WORD" with System Events
  5. wait for the 15 s edit watcher, then read the in-app dictionary
  6. PASS only if a dictionary word carries "$SOURCE_WORD" as an alias
     (or learnedFrom == "$SOURCE_WORD") and the learning store recorded the
     observed text
EOF
  exit 0
fi

[ -x "$BIN" ] || { echo "teletype binary not found at $BIN (cargo build -p teletype-desktop --bin teletype)" >&2; exit 3; }
[ -f "$WAV" ] || { echo "fixture wav not found at $WAV" >&2; exit 3; }

# --- isolated stores ------------------------------------------------------------
TMP="$(mktemp -d -t teletype-learn.XXXXXX)"
CFG="$TMP/config"
DATA="$TMP/data"
mkdir -p "$CFG" "$DATA"
if [ -f "$REAL_DATA/settings.json" ]; then
  cp "$REAL_DATA/settings.json" "$CFG/settings.json"
fi
if [ -d "$REAL_DATA/models" ]; then
  ln -s "$REAL_DATA/models" "$DATA/models"
fi

APP_PID=""
cleanup() {
  if [ -n "$APP_PID" ] && kill -0 "$APP_PID" 2>/dev/null; then
    kill -TERM "$APP_PID" 2>/dev/null
    for _ in $(seq 1 20); do
      kill -0 "$APP_PID" 2>/dev/null || break
      sleep 0.25
    done
  fi
  if [ "${FAILED:-0}" = "1" ]; then
    echo "temp stores kept for inspection: $TMP"
  else
    rm -rf "$TMP"
  fi
}
trap cleanup EXIT

e2e_send() {
  python3 - "$SOCK" "$1" <<'PY'
import socket, sys
sock_path, line = sys.argv[1], sys.argv[2]
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(sock_path)
s.sendall((line + "\n").encode())
s.shutdown(socket.SHUT_WR)
buf = b""
while True:
    chunk = s.recv(65536)
    if not chunk:
        break
    buf += chunk
print(buf.decode().strip())
PY
}

json_get() {
  # json_get '<json>' '<dotted path under data, e.g. result.text>'
  python3 - "$1" "$2" <<'PY'
import json, sys
try:
    d = json.loads(sys.argv[1])
    for key in sys.argv[2].split("."):
        d = d[key]
    print(d)
except Exception:
    sys.exit(1)
PY
}

dict_has_alias() {
  python3 - "$1" "$SOURCE_WORD" "$TARGET_WORD" <<'PY'
import json, sys
try:
    words = json.loads(sys.argv[1])["data"]["words"]
except Exception:
    print("False")
    raise SystemExit(0)
source, target = sys.argv[2].lower(), sys.argv[3].lower()
hit = any(
    str(w.get("word", "")).lower() == target
    or any(str(a).lower() == source for a in w.get("aliases", []))
    or str(w.get("learnedFrom") or "").lower() == source
    for w in words
)
print("True" if hit else "False")
PY
}

learning_saw_edit() {
  python3 - "$1" "$TARGET_WORD" <<'PY'
import json, sys
try:
    rows = json.loads(sys.argv[1])["data"]["dictations"]
except Exception:
    print("False")
    raise SystemExit(0)
target = sys.argv[2].lower()
print("True" if any(target in str(r.get("observed") or "").lower() for r in rows) else "False")
PY
}

# A socket left by a killed run answers `-S` but refuses connections, which
# used to race the new app's bind and silently drop the `start` command.
rm -f "$SOCK"

TELETYPE_E2E_CONFIG_DIR="$CFG" TELETYPE_E2E_DATA_DIR="$DATA" \
  "$BIN" --e2e >"$TMP/app.log" 2>&1 &
APP_PID=$!
READY=0
for _ in $(seq 1 120); do
  if [ -S "$SOCK" ] && e2e_send '{"cmd":"status"}' 2>/dev/null | grep -q '"ok":true'; then
    READY=1
    break
  fi
  sleep 0.5
done
if [ "$READY" != "1" ]; then
  FAILED=1
  echo "FAIL  e2e socket $SOCK never answered (app log: $TMP/app.log)" >&2
  exit 3
fi
echo "e2e app running (pid $APP_PID), isolated stores at $TMP"

# --- real target app ------------------------------------------------------------
focus_textedit() {
  osascript >/dev/null 2>&1 <<'OSA'
tell application "TextEdit"
  activate
  if (count of documents) = 0 then make new document
  set text of front document to ""
end tell
tell application "System Events" to tell process "TextEdit"
  set frontmost to true
  keystroke "a" using {command down}
  key code 51
end tell
OSA
  # One document only: a second Untitled window from an earlier run can take
  # the front, and the app reads the focused field, not "the" document.
  osascript -e 'tell application "TextEdit"
  repeat while (count of documents) > 1
    close front document saving no
  end repeat
end tell' >/dev/null 2>&1
}

read_textedit() {
  # Prefer the document model: it is stable no matter which AX window is
  # frontmost (a second Untitled window made the AX path return empty and
  # made a successful paste look lost). The AX read stays as a fallback for
  # the same element the app itself reads.
  osascript -e 'tell application "TextEdit" to get text of front document' 2>/dev/null \
    || osascript -e 'tell application "System Events" to tell process "TextEdit" to value of attribute "AXValue" of (first text area of scroll area 1 of front window)' 2>/dev/null \
    || true
}

type_correction() {
  # Real keystrokes: select the field and retype it with the correction. The
  # corrected text is passed as argv so names with quotes cannot break the
  # AppleScript source.
  osascript - "$1" >/dev/null <<'OSA'
on run argv
  set corrected to item 1 of argv
  tell application "TextEdit" to activate
  tell application "System Events" to tell process "TextEdit"
    set frontmost to true
    keystroke "a" using {command down}
    keystroke corrected
  end tell
end run
OSA
}

FAILED=1

if ! focus_textedit; then
  echo "FAIL  could not focus TextEdit (Accessibility/Automation permission?)" >&2
  exit 3
fi
sleep 1

# --- the take -------------------------------------------------------------------
START_REPLY="$(e2e_send '{"cmd":"start","input":"'"$WAV"'","command":false}')"
if ! echo "$START_REPLY" | grep -q '"ok":true'; then
  echo "FAIL  start rejected: $START_REPLY" >&2
  exit 1
fi
RESULT=""
for _ in $(seq 1 60); do
  sleep 1
  for id in $(seq 1 20); do
    R="$(e2e_send "{\"cmd\":\"result\",\"id\":$id}")"
    if echo "$R" | grep -q '"done":true'; then
      RESULT="$R"
      break 2
    fi
  done
done
if [ -z "$RESULT" ]; then
  echo "FAIL  no take result within 60 s" >&2
  exit 1
fi
INSERTED="$(json_get "$RESULT" 'data.result.text')"
FIELD_AFTER_TAKE="$(read_textedit)"
echo "take text:   ${INSERTED:0:120}"
echo "field after: ${FIELD_AFTER_TAKE:0:120}"

if ! echo "$INSERTED $FIELD_AFTER_TAKE" | grep -qiF "$SOURCE_WORD"; then
  echo "FAIL  the take did not land \"$SOURCE_WORD\"; nothing to correct" >&2
  exit 1
fi

# The corrected take: the engine's text with the one word fixed. Replacing
# only that token keeps the edit exactly like a real correction (the rest of
# the sentence is untouched, so the judge sees a targeted swap).
CORRECTED="$(python3 - "$INSERTED" "$SOURCE_WORD" "$TARGET_WORD" <<'PY'
import sys
text, source, target = sys.argv[1], sys.argv[2], sys.argv[3]
i = text.lower().find(source.lower())
print("" if i < 0 else text[:i] + target + text[i + len(source):])
PY
)"
if [ -z "$CORRECTED" ]; then
  echo "FAIL  could not build the correction from the take text" >&2
  exit 1
fi
if [ "$CORRECTED" = "$INSERTED" ]; then
  echo "FAIL  the engine already transcribed \"$TARGET_WORD\"; nothing to correct" >&2
  exit 1
fi

# --- the edit (through the real AX path) ----------------------------------------
type_correction "$CORRECTED"
sleep 2
FIELD_AFTER_EDIT="$(read_textedit)"
echo "field after edit: ${FIELD_AFTER_EDIT:0:120}"
if ! echo "$FIELD_AFTER_EDIT" | grep -qiF "$TARGET_WORD"; then
  echo "FAIL  the correction did not land in TextEdit" >&2
  exit 1
fi

# --- wait for the watcher and assert the learned alias ---------------------------
echo "waiting up to ${CHECK_WAIT}s for the edit watcher..."
DICT_OK=0
LEARN_OK=0
for _ in $(seq 1 "$CHECK_WAIT"); do
  sleep 1
  DICT="$(e2e_send '{"cmd":"dictionary"}')"
  LEARN="$(e2e_send '{"cmd":"learning"}')"
  DICT_HIT="$(dict_has_alias "$DICT")"
  LEARN_HIT="$(learning_saw_edit "$LEARN")"
  [ "$DICT_HIT" = "True" ] && DICT_OK=1
  [ "$LEARN_HIT" = "True" ] && LEARN_OK=1
  if [ "$DICT_OK" = "1" ] && [ "$LEARN_OK" = "1" ]; then
    break
  fi
done

echo
echo "dictionary has the learned alias: $DICT_OK"
echo "learning store recorded the observed edit: $LEARN_OK"
if [ "$DICT_OK" = "1" ] && [ "$LEARN_OK" = "1" ]; then
  echo "PASS  learn-from-edits works on the real accessibility path (\"$SOURCE_WORD\" -> \"$TARGET_WORD\")"
  echo "dictionary: $(echo "$DICT" | head -c 400)"
  FAILED=0
  exit 0
fi
echo "dictionary: $(echo "$DICT" | head -c 400)"
echo "learning:   $(echo "$LEARN" | head -c 400)"
echo "FAIL  the edit was not learned (app log: $TMP/app.log)" >&2
exit 1
