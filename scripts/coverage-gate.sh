#!/usr/bin/env bash
# Per-crate coverage gate (plan §9.5).
#
# Runs `cargo llvm-cov --workspace --json --summary-only` and fails when any
# crate's line coverage drops below its per-crate threshold. Thresholds are
# overridable per crate via env vars, e.g. TELETYPE_CORE_MIN_COV=82.
#
# Defaults (current floors, never lower without a ratchet decision):
#   teletype-core    80
#   teletype-speech  70
#   teletype-desktop 55
#
# If cargo-llvm-cov is not installed, prints SKIP and exits 0, unless
# TELETYPE_COVERAGE_STRICT=1, in which case it exits 1 (CI can flip strict
# once the toolchain is guaranteed).

set -euo pipefail

CORE_MIN="${TELETYPE_CORE_MIN_COV:-80}"
SPEECH_MIN="${TELETYPE_SPEECH_MIN_COV:-70}"
DESKTOP_MIN="${TELETYPE_DESKTOP_MIN_COV:-55}"

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  if [[ "${TELETYPE_COVERAGE_STRICT:-0}" == "1" ]]; then
    echo "coverage-gate: cargo-llvm-cov not installed and TELETYPE_COVERAGE_STRICT=1" >&2
    exit 1
  fi
  echo "coverage-gate: SKIP (cargo-llvm-cov not installed; set TELETYPE_COVERAGE_STRICT=1 to make this fatal)"
  exit 0
fi

# --json --summary-only emits one JSON object per file; build a small
# per-crate summary and evaluate thresholds in python3 (always present on
# the CI images this runs on). The evaluator lives in a sibling file so the
# pipe and the script do not fight over stdin.
cargo llvm-cov --workspace --json --summary-only | python3 scripts/coverage-gate-eval.py "$CORE_MIN" "$SPEECH_MIN" "$DESKTOP_MIN"

