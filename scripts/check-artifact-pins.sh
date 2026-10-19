#!/usr/bin/env bash
# EW-04 — pinned-artifact consistency check.
#
# The bundled llama-server binary is pinned to one llama.cpp commit in THREE
# independent places, and nothing until now checked that they agree:
#
#   1. scripts/build-llama-server.sh   COMMIT="f95b0d...c5e28"
#   2. .github/workflows/release.yml   git checkout --detach f95b0d...c5e28
#      (Windows job — a second hardcoded copy that can silently drift)
#   3. THIRD-PARTY-NOTICES.txt         "commit f95b0d...c5e28" header line
#
# If any one moves alone, either the shipped binary disagrees with the
# notices (a license-attribution bug) or macOS and Windows ship different
# code. This script extracts the pinned commit from all three sources and
# fails unless they are equal.
#
# EXTENSION POINT: when another pinned artifact appears (another bundled
# binary, a pinned commit elsewhere), add another row to the extraction
# table below — one extraction function, one source label, one value
# variable — and include it in the comparison.
#
# Exit codes: 0 = all pins consistent, 1 = drift or a missing pin,
#             2 = bad usage (a source file does not exist).
#
# Env overrides (mirrors check-notices.sh) let smoke tests doctor inputs
# without touching the repo:
#   LLAMA_SCRIPT=...      build script (default scripts/build-llama-server.sh)
#   RELEASE_WORKFLOW=...  release workflow (default .github/workflows/release.yml)
#   NOTICES_FILE=...      notices file (default THIRD-PARTY-NOTICES.txt)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT="${LLAMA_SCRIPT:-$ROOT/scripts/build-llama-server.sh}"
WORKFLOW="${RELEASE_WORKFLOW:-$ROOT/.github/workflows/release.yml}"
NOTICES="${NOTICES_FILE:-$ROOT/THIRD-PARTY-NOTICES.txt}"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

for f in "$SCRIPT" "$WORKFLOW" "$NOTICES"; do
  [[ -f "$f" ]] || fail "bad usage: source file not found: $f"
done

# Extraction table: each row is (label, value). A missing match yields an
# empty value, which is a FAIL, not a skip — an unpinned artifact is the
# failure mode this check guards.

# 1. build script: COMMIT="<40-hex>"
script_pin="$(grep -oE 'COMMIT="[0-9a-f]{40}"' "$SCRIPT" | sed -E 's/^COMMIT="([0-9a-f]{40})"$/\1/' | head -n 1 || true)"

# 2. release workflow: every "git checkout --detach <40-hex>" occurrence;
#    more than one DISTINCT value in that file is itself drift.
wf_pins="$(grep -oE 'git checkout --detach [0-9a-f]{40}' "$WORKFLOW" | sed -E 's/^git checkout --detach ([0-9a-f]{40})$/\1/' || true)"
wf_pin=""
wf_distinct="$(printf '%s\n' "$wf_pins" | sed '/^$/d' | sort -u || true)"
if [[ -n "$wf_distinct" ]]; then
  if [[ "$(printf '%s\n' "$wf_distinct" | grep -c .)" -gt 1 ]]; then
    fail "ambiguous pin: multiple distinct commits in $WORKFLOW"
  fi
  wf_pin="$wf_distinct"
fi

# 3. notices: the llama header line "commit <40-hex>"
notices_pin="$(grep -oE 'commit [0-9a-f]{40}' "$NOTICES" | sed -E 's/^commit ([0-9a-f]{40})$/\1/' | head -n 1 || true)"

# --- compare ---
values=("$script_pin" "$wf_pin" "$notices_pin")
paths=("$SCRIPT" "$WORKFLOW" "$NOTICES")

# A missing match is a FAIL, not a skip.
for i in 0 1 2; do
  [[ -n "${values[$i]}" ]] || fail "no pin found in ${paths[$i]}"
done

if [[ "${values[0]}" == "${values[1]}" && "${values[1]}" == "${values[2]}" ]]; then
  echo "OK: llama.cpp pin ${values[0]} consistent across 3 sources (build script, release workflow, notices)"
  exit 0
fi

echo "llama.cpp pin drift:" >&2
for i in 0 1 2; do
  if [[ -n "${values[$i]}" ]]; then
    echo "  ${paths[$i]}: ${values[$i]}" >&2
  else
    echo "  ${paths[$i]}: not found" >&2
  fi
done

# Name the source(s) that deviate from the majority value.
majority=""
for i in 0 1 2; do
  count=0
  for j in 0 1 2; do
    [[ "${values[$i]}" == "${values[$j]}" ]] && count=$((count + 1))
  done
  [[ "$count" -gt 1 ]] && majority="${values[$i]}" && break
done
if [[ -z "$majority" ]]; then
  # All three differ: every source is a mismatch.
  mismatches=("${paths[0]}" "${paths[1]}" "${paths[2]}")
else
  mismatches=()
  for i in 0 1 2; do
    [[ "${values[$i]}" != "$majority" ]] && mismatches+=("${paths[$i]}")
  done
fi
echo "FAIL: llama.cpp pin mismatch: ${mismatches[*]}" >&2
exit 1
