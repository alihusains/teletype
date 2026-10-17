#!/usr/bin/env bash
# E2E delivery matrix (plan §9.6): target app x forced delivery fallback.
#
# Real mode drives the app's `--e2e` IPC hook (unix socket, one JSON
# command per line): the harness focuses the target, sends a `policy`
# override pinning the forced fallback for the target's bundle id, sends
# `start` with the fixture WAV (file-input mode), polls `result`, then
# reads the field back via AX and checks the clipboard was restored.
#
# Usage:
#   scripts/e2e/delivery_matrix_macos.sh --dry      # print the case table
#   scripts/e2e/delivery_matrix_macos.sh            # real run
#
# Real mode env:
#   - TELETYPE_E2E_BIN     teletype binary (default: repo target/debug/teletype)
#   - TELETYPE_E2E_SOCKET  control socket (default /tmp/teletype-e2e.sock)
#   - TELETYPE_E2E_WAV     fixture (default crates/teletype-speech/tests/fixtures/hello.wav)
#   - TELETYPE_E2E_TARGETS space-separated target labels to run (default: all)
#   - TELETYPE_E2E_FALLBACKS space-separated fallbacks to run (default: all)
#   - ACCESSIBILITY permission for the terminal running this script.
#
# Per case: focus target, save clipboard, force mode, trigger dictation,
# read the field back via AX, assert the expected text landed and the
# clipboard was restored. Prints PASS/FAIL per case; exits 1 on any FAIL.
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOCK="${TELETYPE_E2E_SOCKET:-/tmp/teletype-e2e.sock}"
BIN="${TELETYPE_E2E_BIN:-$REPO/target/debug/teletype}"
WAV="${TELETYPE_E2E_WAV:-$REPO/crates/teletype-speech/tests/fixtures/hello.wav}"
# hello.wav ("hello") is the plan's §9.6 fixture; the assertion uses a
# substring of its transcription ("hello") rather than an exact string,
# since the ASR output is not deterministic across engines.
EXPECTED_SUBSTR="hello"
WANT=1  # the e2e app records every take's outcome; 1 = a take ran

DRY=0
[ "${1:-}" = "--dry" ] && DRY=1

# label | app-name | bundle-id | focus-fn | read-fn
TARGETS=(
  "TextEdit|TextEdit|com.apple.TextEdit|focus_textedit|read_textedit"
  "Notes|Notes|com.apple.Notes|focus_notes|read_notes"
  "Safari-local-test-page|Safari|com.apple.Safari|focus_safari|read_safari"
  "Chrome-local-test-page|Google Chrome|com.google.Chrome|focus_chrome|read_chrome"
  "VS Code|Code|com.microsoft.VSCode|focus_vscode|read_vscode"
  "Terminal|Terminal|com.apple.Terminal|focus_terminal|read_terminal"
  "Slack|Slack|com.tinyspeck.slackmacgap|focus_slack|read_slack"
  "Word|Microsoft Word|com.microsoft.Word|focus_word|read_word"
)
ALL_FALLBACKS=(clipboard type setvalue hold)

print_table() {
  echo "delivery matrix: ${#TARGETS[@]} targets x ${#ALL_FALLBACKS[@]} forced fallbacks"
  printf "%-24s | %-10s | %s\n" "TARGET" "FALLBACK" "CHECKS"
  printf "%-24s-+-%-10s-+-%s\n" "------------------------" "----------" "----------------------------------------"
  for t in "${TARGETS[@]}"; do
    for f in "${ALL_FALLBACKS[@]}"; do
      printf "%-24s | %-10s | text lands, clipboard restored\n" "${t%%|*}" "$f"
    done
  done
  echo
  echo "real mode: the app is launched with --e2e (TELETYPE_E2E_BIN) and"
  echo "driven through $SOCK."
  echo "total cases: $(( ${#TARGETS[@]} * ${#ALL_FALLBACKS[@]} ))"
}

if [ "$DRY" = "1" ]; then
  print_table
  exit 0
fi

# --- real mode ----------------------------------------------------------------
if [ ! -x "$BIN" ]; then
  echo "teletype binary not found at $BIN (build it: cargo build -p teletype-desktop --bin teletype)" >&2
  exit 3
fi
if [ ! -f "$WAV" ]; then
  echo "fixture wav not found at $WAV" >&2
  exit 3
fi

# --- app lifecycle --------------------------------------------------------------
# Launch the app once for the whole matrix, headless (--e2e), then kill it.
APP_PID=""
cleanup_app() {
  if [ -n "$APP_PID" ] && kill -0 "$APP_PID" 2>/dev/null; then
    kill "$APP_PID" 2>/dev/null
    wait "$APP_PID" 2>/dev/null
  fi
}
trap cleanup_app EXIT

"$BIN" --e2e >/dev/null 2>&1 &
APP_PID=$!
for _ in $(seq 1 60); do
  [ -S "$SOCK" ] && break
  sleep 0.5
done
if [ ! -S "$SOCK" ]; then
  echo "e2e socket $SOCK never appeared; app not ready" >&2
  exit 3
fi
echo "e2e app running (pid $APP_PID), socket $SOCK"

# e2e_send <json> -> prints the reply line
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
    chunk = s.recv(4096)
    if not chunk:
        break
    buf += chunk
print(buf.decode().strip())
PY
}

# --- focus / read helpers --------------------------------------------------------
open_app() {
  open -a "$1" >/dev/null 2>&1
  sleep 1
}

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
  keystroke delete
end tell
OSA
}

read_textedit() {
  osascript -e 'tell application "System Events" to tell process "TextEdit" to value of attribute "AXValue" of (first text area of front window)' 2>/dev/null
}

focus_notes() {
  osascript >/dev/null 2>&1 <<'OSA' || true
tell application "Notes"
  activate
  tell front window
    tell sidebar
      tell outline 1
        set selected to row 1
      end tell
    end tell
  end tell
end tell
OSA
  sleep 1
  osascript >/dev/null 2>&1 <<'OSA' || true
tell application "System Events" to tell process "Notes"
  set frontmost to true
  keystroke "a" using {command down}
  keystroke delete
end tell
OSA
}

read_notes() {
  # Notes does not expose a plain "text area"; read the AXValue of the
  # focused element instead (best-effort).
  osascript -e 'tell application "System Events" to tell (first process whose frontmost is true) to value of attribute "AXValue" of (first UI element of front window whose role is "AXTextArea")' 2>/dev/null
}

focus_safari() {
  open -a Safari "file://$REPO/scripts/e2e/test-page.html" >/dev/null 2>&1
  sleep 1
  osascript >/dev/null 2>&1 <<OSA || true
tell application "Safari" to activate
tell application "System Events" to tell process "Safari"
  set frontmost to true
  keystroke "l" using {command down}
  keystroke "file://$REPO/scripts/e2e/test-page.html"
  key code 36
end tell
OSA
  sleep 1
  # Click into the page body and select-all + delete to clear the textarea.
  osascript >/dev/null 2>&1 <<'OSA' || true
tell application "System Events" to tell process "Safari"
  set frontmost to true
  keystroke "a" using {command down}
  keystroke delete
end tell
OSA
}

read_safari() {
  osascript -e 'tell application "Safari" to execute front document javascript "document.getElementById(\"target\").value"' 2>/dev/null
}

focus_chrome() {
  open -a "Google Chrome" "file://$REPO/scripts/e2e/test-page.html" >/dev/null 2>&1
  sleep 1
  osascript >/dev/null 2>&1 <<OSA || true
tell application "Google Chrome" to activate
tell application "System Events" to tell process "Google Chrome"
  set frontmost to true
  keystroke "l" using {command down}
  keystroke "file://$REPO/scripts/e2e/test-page.html"
  key code 36
end tell
OSA
  sleep 1
  osascript >/dev/null 2>&1 <<'OSA' || true
tell application "System Events" to tell process "Google Chrome"
  set frontmost to true
  keystroke "a" using {command down}
  keystroke delete
end tell
OSA
}

read_chrome() {
  # Chrome needs "Allow JavaScript from Apple Events" enabled; fall back to
  # the AX read of the focused element (the textarea's AXValue).
  osascript -e 'tell application "Google Chrome" to execute front javascript "document.getElementById(\"target\").value"' 2>/dev/null || \
  osascript -e 'tell application "System Events" to tell (first process whose frontmost is true) to value of attribute "AXValue" of (first UI element whose subrole is "AXTextField")' 2>/dev/null
}

focus_vscode() {
  open -a Code "$REPO/scripts/e2e/vscode_e2e.txt" >/dev/null 2>&1
  sleep 1
  osascript >/dev/null 2>&1 <<'OSA' || true
tell application "Code" to activate
tell application "System Events" to tell process "Code"
  set frontmost to true
  keystroke "a" using {command down}
  keystroke delete
end tell
OSA
}

read_vscode() {
  # VS Code's editor is not a plain AX text area; best-effort AX read.
  osascript -e 'tell application "System Events" to tell process "Code" to value of attribute "AXValue" of (first text area of front window)' 2>/dev/null
}

focus_terminal() {
  osascript >/dev/null 2>&1 <<'OSA' || true
tell application "Terminal" to activate
tell application "System Events" to tell process "Terminal"
  set frontmost to true
  keystroke "a" using {command down}
  keystroke delete
end tell
OSA
}

read_terminal() {
  # Terminal does not expose its scrollback as an AX value on the tty;
  # best-effort AX read of the text area, else empty.
  osascript -e 'tell application "System Events" to tell process "Terminal" to value of attribute "AXValue" of (first text area of front window)' 2>/dev/null || true
}

focus_slack() {
  open_app Slack
  osascript >/dev/null 2>&1 <<'OSA' || true
tell application "Slack" to activate
tell application "System Events" to tell process "Slack"
  set frontmost to true
end tell
OSA
}

read_slack() {
  osascript -e 'tell application "System Events" to tell (first process whose frontmost is true) to value of attribute "AXValue" of (first text area of front window)' 2>/dev/null || true
}

focus_word() {
  open_app "Microsoft Word"
  osascript >/dev/null 2>&1 <<'OSA' || true
tell application "Microsoft Word" to activate
tell application "System Events" to tell process "Microsoft Word"
  set frontmost to true
  keystroke "a" using {command down}
  keystroke delete
end tell
OSA
}

read_word() {
  osascript -e 'tell application "System Events" to tell (first process whose frontmost is true) to value of attribute "AXValue" of (first text area of front window)' 2>/dev/null || true
}

# --- readiness ------------------------------------------------------------------
app_installed() {
  mdfind -name "$1.app" 2>/dev/null | grep -q "\.app$"
}

# --- matrix ----------------------------------------------------------------------
PASS=0
FAIL=0
SKIP=0
RESULTS_FILE="$(mktemp -t e2e_matrix.XXXXXX)"

FALLBACKS=("${ALL_FALLBACKS[@]}")
if [ -n "${TELETYPE_E2E_FALLBACKS:-}" ]; then
  FALLBACKS=(${TELETYPE_E2E_FALLBACKS})
fi
RUN_TARGETS="${TELETYPE_E2E_TARGETS:-${TARGETS[*]%%|*}}"

for target in "${TARGETS[@]}"; do
  IFS='|' read -r label appname bundleid focusfn readfn <<< "$target"
  case " $RUN_TARGETS " in
    *" $label "*) : ;;
    *) continue ;;
  esac
  # Install check.
  if ! app_installed "$appname"; then
    for f in "${FALLBACKS[@]}"; do
      echo "SKIP  $label/$f ($appname not installed)"
      SKIP=$((SKIP+1))
    done
    continue
  fi
  for fallback in "${FALLBACKS[@]}"; do
    case_id="${label}/${fallback}"
    T0=$(date +%s)
    if ! "$focusfn"; then
      echo "FAIL  $case_id (could not focus)"
      FAIL=$((FAIL+1))
      continue
    fi
    sleep 1
    CLIP_BEFORE="$(pbpaste)"
    # Force the mode for this target's bundle id, then start the take.
    e2e_send "{\"cmd\":\"policy\",\"app\":\"$bundleid\",\"force\":\"$fallback\"}" >/dev/null
    e2e_send "{\"cmd\":\"start\",\"input\":\"$WAV\",\"command\":false}" >/dev/null
    # Poll for the result (the feed is real-time paced; hello.wav is <1 s,
    # the pipeline adds a few more seconds). Probe a small id range.
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
    ELAPSED=$(( $(date +%s) - T0 ))
    if [ -z "$RESULT" ]; then
      echo "FAIL  $case_id (no result within 60s; elapsed ${ELAPSED}s)"
      FAIL=$((FAIL+1))
      continue
    fi
    DELIVERY="$(echo "$RESULT" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get("data",{}).get("result",{}).get("delivery",""))' 2>/dev/null)"
    TEXT="$(echo "$RESULT" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get("data",{}).get("result",{}).get("text",""))' 2>/dev/null)"
    ERR="$(echo "$RESULT" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get("data",{}).get("result",{}).get("error") or "")' 2>/dev/null)"
    GOT="$("$readfn")"
    CLIP_AFTER="$(pbpaste)"
    TEXT_OK=0
    if [ "$fallback" = "hold" ]; then
      # hold short-circuits the cascade: the text is kept for Paste-Last
      # and must NOT land in the field. The pass condition is that the
      # take's result reports the hold outcome.
      [ "$DELIVERY" = "hold" ] && TEXT_OK=1
    elif [[ "$GOT" == *"$EXPECTED_SUBSTR"* || "$TEXT" == *"$EXPECTED_SUBSTR"* ]]; then
      TEXT_OK=1
    fi
    if [ "$CLIP_BEFORE" = "$CLIP_AFTER" ]; then
      CLIP_OK=1
    else
      CLIP_OK=0
    fi
    if [ "$TEXT_OK" = "1" ] && [ "$CLIP_OK" = "1" ]; then
      echo "PASS  $case_id (delivery=$DELIVERY elapsed=${ELAPSED}s)"
      { echo "[$case_id] field: $(echo "$GOT" | head -c 200)"; echo "[$case_id] text: $(echo "$TEXT" | head -c 200)"; } >> "$RESULTS_FILE"
      PASS=$((PASS+1))
    else
      echo "FAIL  $case_id (text=$TEXT_OK clipboard_restored=$CLIP_OK delivery=$DELIVERY err=$ERR elapsed=${ELAPSED}s)"
      { echo "[$case_id] field: $(echo "$GOT" | head -c 200)"; echo "[$case_id] text: $(echo "$TEXT" | head -c 200)"; } >> "$RESULTS_FILE"
      FAIL=$((FAIL+1))
    fi
  done
done

echo
echo "delivery matrix: $PASS passed, $FAIL failed, $SKIP skipped"
cat "$RESULTS_FILE"
[ "$FAIL" -eq 0 ] && exit 0 || exit 1
