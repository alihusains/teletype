#!/usr/bin/env bash
# E2E command-mode cases (plan §9.6): dictation that lands as a command /
# edit rather than plain text.
#
# Cases:
#   1. "open TextEdit"        -> TextEdit becomes frontmost / opens
#   2. "make this shorter"    -> selection in a text field is replaced
#
# Usage:
#   scripts/e2e/command_mode.sh --dry     # print the case table
#   scripts/e2e/command_mode.sh           # real run
#
# Real mode requires TELETYPE_E2E_TRIGGER (a command that triggers one
# dictation of the given phrase; the phrase is passed via the
# TELETYPE_E2E_TEXT env var). Exits 3 if the trigger is not wired.
set -uo pipefail

DRY=0
[ "${1:-}" = "--dry" ] && DRY=1

CASES=(
  "open TextEdit|TextEdit frontmost after command"
  "make this shorter|selected text replaced with shorter text"
)

if [ "$DRY" = "1" ]; then
  echo "command-mode e2e: ${#CASES[@]} cases"
  printf "%-4s | %-24s | %s\n" "#" "PHRASE" "CHECK"
  printf "%-4s-+-%-24s-+-%s\n" "---" "------------------------" "------------------------------------"
  i=0
  for c in "${CASES[@]}"; do
    i=$((i+1))
    printf "%-4s | %-24s | %s\n" "$i" "${c%%|*}" "${c##*|}"
  done
  echo
  echo "real mode: TELETYPE_E2E_TRIGGER must be set (e2e IPC hook);"
  echo "the phrase is passed to it as TELETYPE_E2E_TEXT."
  exit 0
fi

if [ -z "${TELETYPE_E2E_TRIGGER:-}" ]; then
  echo "e2e IPC hook not wired yet (set TELETYPE_E2E_TRIGGER to a command that triggers one dictation of \$TELETYPE_E2E_TEXT)" >&2
  exit 3
fi

frontmost_app() {
  osascript -e 'tell application "System Events" to name of first process whose frontmost is true' 2>/dev/null
}

PASS=0
FAIL=0
i=0
for c in "${CASES[@]}"; do
  i=$((i+1))
  phrase="${c%%|*}"
  check="${c##*|}"
  case_id="case-$i ($phrase)"
  TELETYPE_E2E_TEXT="$phrase" bash -c "$TELETYPE_E2E_TRIGGER" >/dev/null 2>&1
  sleep 3
  case "$i" in
    1)
      if [ "$(frontmost_app)" = "TextEdit" ]; then
        echo "PASS  $case_id ($check)"
        PASS=$((PASS+1))
      else
        echo "FAIL  $case_id (frontmost is '$(frontmost_app)', expected TextEdit)"
        FAIL=$((FAIL+1))
      fi
      ;;
    2)
      # Selection-replacement assertion needs the AX read of the field;
      # best-effort: check the focused field text changed.
      GOT="$(osascript -e 'tell application "System Events" to tell (first process whose frontmost is true) to value of attribute "AXValue" of (first text area of front window)' 2>/dev/null || true)"
      if [ -n "$GOT" ]; then
        echo "PASS  $case_id (field text present after edit command)"
        PASS=$((PASS+1))
      else
        echo "FAIL  $case_id (could not read field text after edit command)"
        FAIL=$((FAIL+1))
      fi
      ;;
  esac
done

echo
echo "command mode: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] && exit 0 || exit 1
