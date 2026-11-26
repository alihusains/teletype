#!/usr/bin/env bash
# EW-04 — pinned-artifact consistency check.
#
# The bundled llama-server binary is pinned to one llama.cpp commit in THREE
# independent places, and its Windows prebuilt is additionally identified by a
# release tag repeated in the same three places — four facts, and nothing until
# now checked that they agree:
#
#   1. scripts/build-llama-server.sh   COMMIT="<40-hex>", LLAMA_RELEASE_TAG="b<num>"
#   2. .github/workflows/release.yml   "# llama.cpp pin: <40-hex>" and
#      "# llama.cpp release tag: b<num>" comments (the Windows job downloads
#      the official prebuilt by tag; these are the other hardcoded copies)
#   3. THIRD-PARTY-NOTICES.txt         "commit <40-hex>" and
#      "llama.cpp release tag b<num>" lines
#
# If any one moves alone, either the shipped binary disagrees with the
# notices (a license-attribution bug) or macOS and Windows ship different
# code: bumping only the release tag used to pass every check while Windows
# downloaded a different llama-server. This script extracts the pinned commit
# AND the release tag from all three sources and fails unless they are equal.
#
# It also checks the model the release smoke stages: the URL in release.yml
# must be exactly `<PARAKEET_BASE_URL>/<file>` from the speech catalog. That
# URL once pointed at the whisper.cpp repo, 404'd, and failed the whole
# v0.2.7 release; the catalog and the workflow are two hardcoded copies of
# the same fact, so they are pinned to each other here.
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
#   SPEECH_CATALOG=...    speech catalog (default crates/teletype-speech/src/catalog.rs)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT="${LLAMA_SCRIPT:-$ROOT/scripts/build-llama-server.sh}"
WORKFLOW="${RELEASE_WORKFLOW:-$ROOT/.github/workflows/release.yml}"
NOTICES="${NOTICES_FILE:-$ROOT/THIRD-PARTY-NOTICES.txt}"
SPEECH_CATALOG="${SPEECH_CATALOG:-$ROOT/crates/teletype-speech/src/catalog.rs}"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

for f in "$SCRIPT" "$WORKFLOW" "$NOTICES" "$SPEECH_CATALOG"; do
  [[ -f "$f" ]] || fail "bad usage: source file not found: $f"
done

# Extraction table: each row is (label, value). A missing match yields an
# empty value, which is a FAIL, not a skip — an unpinned artifact is the
# failure mode this check guards.

# 1. build script: COMMIT="<40-hex>"
script_pin="$(grep -oE 'COMMIT="[0-9a-f]{40}"' "$SCRIPT" | sed -E 's/^COMMIT="([0-9a-f]{40})"$/\1/' | head -n 1 || true)"

# 2. release workflow: every "# llama.cpp pin: <40-hex>" occurrence; more
#    than one DISTINCT value in that file is itself drift.
wf_pins="$(grep -oE 'llama\.cpp pin: [0-9a-f]{40}' "$WORKFLOW" | sed -E 's/^llama\.cpp pin: ([0-9a-f]{40})$/\1/' || true)"
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

# 4. Windows prebuilt release tag. Extracted through explicit markers, not a
#    broad b<num> regex: the build script's prose mentions historical b11960 /
#    b11488, which a loose pattern would pick up.
script_tag="$(grep -oE 'LLAMA_RELEASE_TAG="b[0-9]+"' "$SCRIPT" | sed -E 's/^LLAMA_RELEASE_TAG="(b[0-9]+)"$/\1/' | head -n 1 || true)"
wf_tag="$(grep -oE 'llama\.cpp release tag: b[0-9]+' "$WORKFLOW" | sed -E 's/^llama\.cpp release tag: (b[0-9]+)$/\1/' | head -n 1 || true)"
notices_tag="$(grep -oE 'llama\.cpp release tag b[0-9]+' "$NOTICES" | sed -E 's/^llama\.cpp release tag (b[0-9]+)$/\1/' | head -n 1 || true)"

# --- compare ---
# Both comparisons run; a drifting commit must not hide a drifting tag.
pin_failed=0
values=("$script_pin" "$wf_pin" "$notices_pin")
paths=("$SCRIPT" "$WORKFLOW" "$NOTICES")

# A missing match is a FAIL, not a skip.
for i in 0 1 2; do
  [[ -n "${values[$i]}" ]] || fail "no pin found in ${paths[$i]}"
done

if [[ "${values[0]}" == "${values[1]}" && "${values[1]}" == "${values[2]}" ]]; then
  echo "OK: llama.cpp pin ${values[0]} consistent across 3 sources (build script, release workflow, notices)"
else
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
  pin_failed=1
fi

# --- Windows prebuilt release tag: same three sources ----------------------
tag_values=("$script_tag" "$wf_tag" "$notices_tag")

# A missing marker is a FAIL, not a skip: an untagged artifact is exactly the
# drift this check guards against.
for i in 0 1 2; do
  [[ -n "${tag_values[$i]}" ]] || fail "no llama.cpp release tag found in ${paths[$i]}"
done

if [[ "${tag_values[0]}" == "${tag_values[1]}" && "${tag_values[1]}" == "${tag_values[2]}" ]]; then
  echo "OK: llama.cpp release tag ${tag_values[0]} consistent across 3 sources (build script, release workflow, notices)"
else
  echo "llama.cpp release tag drift:" >&2
  for i in 0 1 2; do
    echo "  ${paths[$i]}: ${tag_values[$i]}" >&2
  done
  tag_mismatches=()
  for i in 1 2; do
    if [[ "${tag_values[$i]}" != "${tag_values[0]}" ]]; then
      tag_mismatches+=("${paths[$i]}")
    fi
  done
  if [[ "${#tag_mismatches[@]}" -eq 0 ]]; then
    tag_mismatches=("${paths[0]}")
  fi
  echo "FAIL: llama.cpp release tag mismatch: ${tag_mismatches[*]}" >&2
  pin_failed=1
fi

[[ "$pin_failed" -eq 0 ]] || exit 1

# --- speech model download URL: release smoke vs catalog -------------------
base_url="$(grep -oE 'const PARAKEET_BASE_URL: &str = "[^"]+"' "$SPEECH_CATALOG" \
  | sed -E 's/.*"(.*)"$/\1/' | head -n 1 || true)"
smoke_file="$(grep -oE 'file: "ggml-parakeet-tdt-0\.6b-v3-q8_0\.bin"' "$SPEECH_CATALOG" \
  | sed -E 's/^file: "(.*)"$/\1/' | head -n 1 || true)"
[[ -n "$base_url" ]] || fail "no PARAKEET_BASE_URL in $SPEECH_CATALOG"
[[ -n "$smoke_file" ]] || fail "no ggml-parakeet-tdt-0.6b-v3-q8_0.bin entry in $SPEECH_CATALOG"

expected_url="${base_url}/${smoke_file}"
wf_model_url="$(grep -oE 'https://[^" ]+/ggml-parakeet-tdt-0\.6b-v3-q8_0\.bin' "$WORKFLOW" \
  | head -n 1 || true)"
[[ -n "$wf_model_url" ]] || fail "release smoke does not stage $smoke_file"

if [[ "$wf_model_url" == "$expected_url" ]]; then
  echo "OK: release smoke stages $smoke_file from the catalog base URL"
else
  fail "release smoke model URL drift: workflow=$wf_model_url catalog=$expected_url"
fi

# The smoke must pin the row it staged: the platform default on Apple
# Silicon resolves to Core ML, which the staged artifact dir never holds.
grep -q 'TELETYPE_SELF_TEST_MODEL=parakeet-tdt-v3' "$WORKFLOW" \
  || fail "macOS smoke does not pin TELETYPE_SELF_TEST_MODEL=parakeet-tdt-v3"

# --- Tauri plugin version consistency: JS (npm) vs Rust (cargo) -----------
#
# The JS and Rust sides of each Tauri plugin must be the same major.minor.patch.
# A mismatch on the updater plugin has direct user impact (the JS check()/
# download path talks to the Rust command surface). All other plugin pairs
# should also be aligned to prevent silent drift on the next npm/cargo update.
#
# Env overrides:
#   UI_LOCKFILE=...    npm lockfile (default ui/package-lock.json)
#   CARGO_LOCKFILE=... cargo lockfile (default Cargo.lock)

UI_LOCK="${UI_LOCKFILE:-$ROOT/ui/package-lock.json}"
CARGO_LOCK="${CARGO_LOCKFILE:-$ROOT/Cargo.lock}"

for f in "$UI_LOCK" "$CARGO_LOCK"; do
  [[ -f "$f" ]] || fail "bad usage: lockfile not found: $f"
done

# Each row: (npm_package, cargo_crate).
# The version is extracted as the first major.minor.patch match.
plugin_pairs=(
  "@tauri-apps/api|tauri"
  "@tauri-apps/plugin-dialog|tauri-plugin-dialog"
  "@tauri-apps/plugin-opener|tauri-plugin-opener"
  "@tauri-apps/plugin-process|tauri-plugin-process"
  "@tauri-apps/plugin-updater|tauri-plugin-updater"
)

plugin_failed=0
for pair in "${plugin_pairs[@]}"; do
  npm_pkg="${pair%%|*}"
  cargo_crate="${pair##*|}"

  # Extract the version from the npm lockfile.
  # The lockfile stores packages under "node_modules/<name>" with a "version" field.
  npm_ver="$(awk -v p="node_modules/${npm_pkg}\"" '$0 ~ p {found=1; next} found && /"version":/ {print; exit}' "$UI_LOCK" 2>/dev/null | sed -E 's/.*"version": "([0-9]+\.[0-9]+\.[0-9]+).*/\1/' || true)"

  # Extract the version from the Cargo lockfile.
  cargo_ver="$(awk -v c="${cargo_crate}" '/^name = /{n=$0} n ~ "name = \"" c "\"$" && /^version = /{print; exit}' "$CARGO_LOCK" 2>/dev/null | sed -E 's/.*version = \"([0-9]+\.[0-9]+\.[0-9]+).*/\1/' || true)"

  if [[ -z "$npm_ver" || -z "$cargo_ver" ]]; then
    echo "WARN: could not extract version for ${npm_pkg} / ${cargo_crate} (npm=${npm_ver:-missing} cargo=${cargo_ver:-missing})" >&2
    continue
  fi

  if [[ "$npm_ver" == "$cargo_ver" ]]; then
    echo "OK: ${npm_pkg} / ${cargo_crate} aligned at ${npm_ver}"
  else
    echo "FAIL: ${npm_pkg} (npm ${npm_ver}) != ${cargo_crate} (cargo ${cargo_ver})" >&2
    plugin_failed=1
  fi
done

[[ "$plugin_failed" -eq 0 ]] || exit 1
