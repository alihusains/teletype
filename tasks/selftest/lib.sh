#!/usr/bin/env bash
# Shared helpers for the selftest scripts. Sourced, never executed directly.
# Nothing in here ever writes dictated text, screen text or field text to disk,
# logs or stdout: lengths and counts only.

# Frontmost application name via System Events.
frontmost_app() {
  osascript -e 'tell application "System Events" to get name of first process whose frontmost is true' 2>/dev/null || true
}

# Open a brand new TextEdit document and make TextEdit frontmost.
# Mirrors run_take.sh exactly.
open_textedit() {
  osascript -e 'tell application "TextEdit" to make new document' >/dev/null
  open -a TextEdit
  sleep 1.5
}

# PID of the running Teletype app (first match) or empty. Tries, in order:
# explicit TELETYPE_APP_PID, TELETYPE_PGREP_PATTERNS (colon separated, each
# pattern tried in turn), the running app bundle name, and finally the
# teletype-desktop binary (dev builds).
app_pid() {
  local pats p old b
  if [ -n "${TELETYPE_APP_PID:-}" ]; then
    if ps -p "${TELETYPE_APP_PID}" >/dev/null 2>&1; then
      printf '%s' "$TELETYPE_APP_PID"
      return 0
    fi
    echo "warning: TELETYPE_APP_PID=${TELETYPE_APP_PID} not alive; falling through" >&2
  fi
  old="$IFS"
  pats="${TELETYPE_PGREP_PATTERNS:-Teletype.app/Contents/MacOS/:teletype-desktop}"
  IFS=':'
  set -- $pats
  IFS="$old"
  for p in "$@"; do
    # pgrep only takes one pattern per call, so try each pattern in turn.
    p=$(pgrep -f "$p" 2>/dev/null | head -1 || true)
    if [ -n "$p" ]; then
      printf '%s' "$p"
      return 0
    fi
  done
  # Bundle names may differ from "Teletype" (localizations, test builds);
  # look for any bundle whose binary path contains teletype.
  for b in $(pgrep -f "/Applications/.*/Contents/MacOS/" 2>/dev/null); do
    p=$(ps -o command= -p "$b" 2>/dev/null | grep -ci teletype || true)
    if [ "${p:-0}" -gt 0 ]; then
      printf '%s' "$b"
      return 0
    fi
  done
  return 1
}

# Resident set size of a PID in KB, or empty.
rss_kb() {
  ps -o rss= -p "$1" 2>/dev/null | awk 'NR==1{print $1}' || true
}

# Number of regular files in the recovery spool
# (~/Library/Application Support/com.teletype.app/recovery/).
recovery_count() {
  local dir n
  dir="${TELETYPE_APP_SUPPORT:-$HOME/Library/Application Support/com.teletype.app}"
  n=$(find "$dir/recovery" -maxdepth 1 -type f 2>/dev/null | wc -l | tr -d ' ')
  printf '%s' "${n:-0}"
}

# Path of the latest take trace under the traces dir.
latest_trace() {
  local dir f
  dir="${TELETYPE_TRACES:-$HOME/Library/Application Support/com.teletype.app/traces}"
  f=$(ls -t "$dir" 2>/dev/null | head -1 || true)
  if [ -z "$f" ]; then
    return 1
  fi
  printf '%s/%s' "$dir" "$f"
}

# Count fixed-string occurrences of $1 in the current TextEdit text.
# Never prints the text itself, only the count.
count_fragment() {
  local frag text
  frag="$1"
  text=$(ocu call get_app_state --args '{"app":"TextEdit","text_limit":600}' 2>/dev/null || true)
  printf '%s' "$text" | grep -oiF -- "$frag" 2>/dev/null | wc -l | tr -d ' '
}

# Names of overlay windows worth watching for a stuck pill.
# Best effort: the app may show none, which is not a failure by itself.
pill_windows() {
  osascript -e 'tell application "System Events" to tell process "Teletype" to get name of every window' 2>/dev/null || true
}
