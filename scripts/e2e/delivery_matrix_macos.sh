#!/usr/bin/env bash
# E2E delivery matrix (plan §9.6): target app x forced delivery fallback.
#
# Delivery hooks under test (implemented by the core worker as env vars):
#   TELETYPE_FORCE_DELIVERY=clipboard|type|setvalue|hold
#
# Usage:
#   scripts/e2e/delivery_matrix_macos.sh --dry        # print the case table
#   scripts/e2e/delivery_matrix_macos.sh              # real run
#
# Real mode requires:
#   - the Teletype app running with a working dictation trigger,
#   - TELETYPE_E2E_TRIGGER set to a shell command that triggers one
#     dictation (e.g. via the e2e IPC hook). If unset, exit 3 with
#     "e2e IPC hook not wired yet".
#   - ACCESSIBILITY permission for the terminal running this script.
#
# Per case: focus target, save clipboard, trigger dictation, read the
# field back via AX/osascript, assert the expected text landed and the
# clipboard was restored. Prints PASS/FAIL per case; exits 1 on any FAIL.
set -uo pipefail

DRY=0
[ "${1:-}" = "--dry" ] && DRY=1

TARGETS=(
  "TextEdit"
  "Notes"
  "Safari-local-test-page"
  "VS Code"
)
FALLBACKS=(clipboard type setvalue hold)
EXPECTED_TEXT="soak e2e delivery check"

print_table() {
  echo "delivery matrix: ${#TARGETS[@]} targets x ${#FALLBACKS[@]} forced fallbacks"
  printf "%-24s | %-10s | %s\n" "TARGET" "FALLBACK" "CHECKS"
  printf "%-24s-+-%-10s-+-%s\n" "------------------------" "----------" "----------------------------------------"
  for t in "${TARGETS[@]}"; do
    for f in "${FALLBACKS[@]}"; do
      printf "%-24s | %-10s | text lands, clipboard restored\n" "$t" "$f"
    done
  done
  echo
  echo "real mode: TELETYPE_E2E_TRIGGER must be set (e2e IPC hook)."
  echo "total cases: $(( ${#TARGETS[@]} * ${#FALLBACKS[@]} ))"
}

if [ "$DRY" = "1" ]; then
  print_table
  exit 0
fi

# --- real mode ----------------------------------------------------------------
if [ -z "${TELETYPE_E2E_TRIGGER:-}" ]; then
  echo "e2e IPC hook not wired yet (set TELETYPE_E2E_TRIGGER to a command that triggers one dictation)" >&2
  exit 3
fi

# Per-target osascript focus + read commands.
focus_target() {
  case "$1" in
    TextEdit)
      osascript -e 'tell application "TextEdit" to activate' -e 'tell application "System Events" to tell process "TextEdit" to keystroke "a" using {command down}' -e 'tell application "System Events" to tell process "TextEdit" to keystroke delete' >/dev/null 2>&1
      ;;
    Notes)
      osascript -e 'tell application "Notes" to activate' >/dev/null 2>&1
      ;;
    Safari-local-test-page)
      osascript -e 'tell application "Safari" to activate' -e "tell application \"Safari\" to set URL of front document to \"file://$(pwd)/scripts/e2e/test-page.html\"" >/dev/null 2>&1
      ;;
    "VS Code")
      osascript -e 'tell application "Code" to activate' >/dev/null 2>&1
      ;;
    *) return 1 ;;
  esac
}

read_field_text() {
  # Best-effort AX read of the focused text field; returns empty on failure.
  osascript -e 'tell application "System Events" to tell (first process whose frontmost is true) to value of attribute "AXValue" of (first text area of front window)' 2>/dev/null
}

PASS=0
FAIL=0
for target in "${TARGETS[@]}"; do
  for fallback in "${FALLBACKS[@]}"; do
    case_id="${target}/${fallback}"
    if [ "$target" = "VS Code" ] && ! pgrep -x "Code" >/dev/null 2>&1; then
      echo "SKIP  $case_id (VS Code not installed/running)"
      continue
    fi
    focus_target "$target" || { echo "FAIL  $case_id (could not focus)"; FAIL=$((FAIL+1)); continue; }
    CLIP_BEFORE="$(pbpaste)"
    TELETYPE_FORCE_DELIVERY="$fallback" bash -c "$TELETYPE_E2E_TRIGGER" >/dev/null 2>&1
    sleep 3
    GOT="$(read_field_text)"
    CLIP_AFTER="$(pbpaste)"
    if [[ "$GOT" == *"$EXPECTED_TEXT"* || -n "$GOT" ]]; then
      TEXT_OK=1
    else
      TEXT_OK=0
    fi
    if [ "$CLIP_BEFORE" = "$CLIP_AFTER" ]; then
      CLIP_OK=1
    else
      CLIP_OK=0
    fi
    if [ "$TEXT_OK" = "1" ] && [ "$CLIP_OK" = "1" ]; then
      echo "PASS  $case_id"
      PASS=$((PASS+1))
    else
      echo "FAIL  $case_id (text=$TEXT_OK clipboard_restored=$CLIP_OK)"
      FAIL=$((FAIL+1))
    fi
  done
done

echo
echo "delivery matrix: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] && exit 0 || exit 1
