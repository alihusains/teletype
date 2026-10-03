#!/usr/bin/env bash
# Teletype QA harness — the end-to-end gate before a release or merge.
#
# Runs the full verification stack and prints a single PASS/FAIL summary.
# Usage: scripts/qa.sh [--fast]
#   --fast  skip the release build (dev-profile checks only) for quick iteration
#
# Exits 0 only if every stage passes. Designed to be run by a human or by an
# agent (pi) as the final "is this shippable?" check.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"
export NODE_OPTIONS="--use-system-ca"

FAST=0
[ "${1:-}" = "--fast" ] && FAST=1

PASS=0
FAIL=0
FAILED_STAGES=()

stage() {
  local name="$1"; shift
  echo
  echo "==================================================================="
  echo "STAGE: $name"
  echo "==================================================================="
  if "$@"; then
    echo "  [PASS] $name"
    PASS=$((PASS+1))
  else
    echo "  [FAIL] $name"
    FAIL=$((FAIL+1))
    FAILED_STAGES+=("$name")
  fi
}

# --- Stage 1: Rust format (must match CI's `cargo fmt --all --check`) ------
rust_fmt() {
  if cargo fmt --all -- --check; then
    echo "  fmt: clean"
  else
    echo "  [FAIL] fmt drift (run `cargo fmt --all` to fix)"
    return 1
  fi
}

# --- Stage 2: clippy (must match CI: zero lint warnings via the shared filter)
rust_clippy() {
  # Same command + filter CI runs, so a warning CI would catch is caught here
  # first. The filter (scripts/check-clippy-clean.py) treats real lints as
  # fatal and clang build-script linker noise as non-fatal.
  #
  # Stderr is deliberately not discarded: it carries the real reason clippy
  # failed (missing component, compile error). Dropping it turns every failure
  # into an unexplained exit code, and it also lets the filter see an empty
  # stream, which is how a local box without the clippy component used to
  # report "clippy: clean" while linting nothing.
  set -o pipefail
  cargo clippy --workspace --all-targets --message-format=json \
    | python3 scripts/check-clippy-clean.py
}

# --- Stage 3: full Rust test suite -----------------------------------------
rust_tests() {
  cargo test --workspace 2>&1 | grep -E "test result:|running [0-9]+ test|error\[|panicked" | tail -40
  cargo test --workspace >/dev/null 2>&1
}

# --- Stage 4: UI typecheck --------------------------------------------------
ui_typecheck() {
  (cd ui && npx tsc --noEmit 2>&1) | tail -20
  (cd ui && npx tsc --noEmit) >/dev/null 2>&1
}

# --- Stage 5: UI production build ------------------------------------------
ui_build() {
  (cd ui && npm run build 2>&1) | tail -8
  (cd ui && npm run build) >/dev/null 2>&1
}

# --- Stage 6: release build (skipped with --fast) ---------------------------
release_build() {
  if [ "$FAST" = "1" ]; then
    echo "  (skipped: --fast)"
    return 0
  fi
  cargo build --release --workspace 2>&1 | tail -8
  cargo build --release --workspace >/dev/null 2>&1
}

# --- Stage 7: secret scan (no committed credentials) ------------------------
secret_scan() {
  echo "  scanning tracked files for obvious secrets..."
  if git grep -nE "(sk-[A-Za-z0-9]{20,}|AKIA[0-9A-Z]{16}|-----BEGIN (RSA|OPENSSH) PRIVATE KEY|ghp_[A-Za-z0-9]{36})" -- . ':!ew/' 2>/dev/null; then
    echo "  [FAIL] possible secret found above"
    return 1
  fi
  echo "  no obvious secrets in tracked files"
  return 0
}

# --- Stage 8: ew/ reference not leaked into the build -----------------------
ew_leak_scan() {
  echo "  checking no ew/ reference files are tracked..."
  if git ls-files ew/ | grep -q .; then
    echo "  [FAIL] ew/ reference files are tracked (must stay gitignored)"
    return 1
  fi
  echo "  ew/ is not tracked"
  return 0
}

echo "Teletype QA harness  (fast=$FAST)"
echo "repo: $REPO"
echo "git:  $(git log --oneline -1 2>/dev/null)"

stage "rust fmt (must be clean)" rust_fmt
stage "clippy (must be clean)" rust_clippy
stage "rust tests (workspace)" rust_tests
stage "ui typecheck (tsc)" ui_typecheck
stage "ui build (vite)" ui_build
stage "release build" release_build
stage "secret scan" secret_scan
stage "ew/ leak scan" ew_leak_scan

echo
echo "==================================================================="
echo "QA SUMMARY: $PASS passed, $FAIL failed"
if [ "$FAIL" -gt 0 ]; then
  echo "Failed stages: ${FAILED_STAGES[*]}"
  echo "RESULT: FAIL"
  echo "==================================================================="
  exit 1
fi
echo "RESULT: PASS — shippable"
echo "==================================================================="
exit 0
