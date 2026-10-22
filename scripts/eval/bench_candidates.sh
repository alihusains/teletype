#!/usr/bin/env bash
# ASR candidate benchmark: the models we ship vs external candidates.
#
# Phase 1 — baseline: every catalog engine that can load on this host runs
#   the eval sets through the Rust harness (`teletype-eval run --engine …`).
# Phase 2 — candidates: an external model (AuroraASR-1M, Nemotron streaming,
#   Qualcomm Zipformer) transcribes the same audio in Python, its hypotheses
#   are scored by the *same* Rust harness in replay mode, so WER, the
#   verbatim normalizer and the report format are identical for every engine.
#
# Usage:
#   scripts/eval/bench_candidates.sh [--sets a,b,c] [--engines e1,e2]
#                                    [--candidates aurora,nemotron,zipformer]
#                                    [--out-root <dir>] [--skip-baseline]
#
# Resumable: a cell whose report.json already exists is skipped.
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT" || exit 1

MODELS_DIR="${TELETYPE_MODELS_DIR:-$HOME/Library/Application Support/com.teletype.app/models}"
OUT_ROOT="${REPO_ROOT}/evals/runs/bench-2026-10-21"
EVAL_BIN="${REPO_ROOT}/target/release/teletype-eval"
PY="${TELETYPE_ASR_PYTHON:-${REPO_ROOT}/.tmp/asrbench/.venv/bin/python}"
BENCH="${REPO_ROOT}/scripts/eval/bench_candidate_asr.py"

# The eval sets the §3.8 bake-off covers. `human` is not in the public
# corpus cache, so it is not listed here (the harness skips missing sets).
SETS="${SETS:-librispeech-clean,librispeech-other,earnings22-vocab,ami,cv,noise-10db,noise-5db,noise-0db,fleurs-en}"
ENGINES="${ENGINES:-parakeet-ultra-coreml,parakeet-tdt-v3,sherpa-parakeet-v3-int8,sherpa-parakeet-v2-int8,large-v3-turbo-q5}"
CANDIDATES="${CANDIDATES:-aurora,nemotron,zipformer}"
SKIP_BASELINE=0
BOOST=off

while [[ $# -gt 0 ]]; do
  case "$1" in
    --sets) SETS="$2"; shift 2 ;;
    --engines) ENGINES="$2"; shift 2 ;;
    --candidates) CANDIDATES="$2"; shift 2 ;;
    --out-root) OUT_ROOT="$2"; shift 2 ;;
    --models-dir) MODELS_DIR="$2"; shift 2 ;;
    --skip-baseline) SKIP_BASELINE=1; shift ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

if [[ ! -x "$EVAL_BIN" ]]; then
  echo "building teletype-eval (release, LTO off)..."
  cargo build --release -p teletype-eval --config profile.release.lto=false || exit 1
fi

mkdir -p "$OUT_ROOT"
LOG="${OUT_ROOT}/bench.log"
: > "$LOG"

say() { echo "$*" | tee -a "$LOG"; }

say "=== ASR candidate benchmark ==="
say "date:      $(date -u +%Y-%m-%dT%H:%M:%SZ)"
say "models:    ${MODELS_DIR}"
say "out root:  ${OUT_ROOT}"
say "sets:      ${SETS}"
say "engines:   ${ENGINES}"
say "candidates:${CANDIDATES}"
say "boost:     ${BOOST}"
say ""

fail=0

# ── Phase 1: baseline (the models we have) ─────────────────────────────────
if [[ $SKIP_BASELINE -eq 0 ]]; then
  say "--- phase 1: baseline engines ---"
  for engine in ${ENGINES//,/ }; do
    for set in ${SETS//,/ }; do
      cell="${OUT_ROOT}/baseline/${engine}/${set}-${BOOST}"
      if [[ -f "${cell}/report.json" ]]; then
        say "  skip   ${engine} / ${set} (already done)"
        continue
      fi
      say "  run    ${engine} / ${set}"
      if ! "$EVAL_BIN" run \
          --engine "$engine" \
          --set "$set" \
          --boost "$BOOST" \
          --models-dir "$MODELS_DIR" \
          --out "$cell" >>"$LOG" 2>&1; then
        say "  FAILED ${engine} / ${set}"
        fail=$((fail + 1))
      fi
    done
  done
  say ""
fi

# ── Phase 2: external candidates ───────────────────────────────────────────
say "--- phase 2: external candidates ---"
for cand in ${CANDIDATES//,/ }; do
  for set in ${SETS//,/ }; do
    cell="${OUT_ROOT}/candidates/${cand}/${set}-${BOOST}"
    if [[ -f "${cell}/report.json" ]]; then
      say "  skip   ${cand} / ${set} (already done)"
      continue
    fi
    say "  asr    ${cand} / ${set}"
    if ! "$PY" "$BENCH" \
        --model "$cand" \
        --set "$set" \
        --out "${cell}/hypotheses.jsonl" >>"$LOG" 2>&1; then
      say "  FAILED (asr) ${cand} / ${set}"
      fail=$((fail + 1))
      continue
    fi
    say "  score  ${cand} / ${set}"
    if ! "$EVAL_BIN" run \
        --engine none \
        --set "$set" \
        --hypotheses "${cell}/hypotheses.jsonl" \
        --model-label "$cand" \
        --out "$cell" >>"$LOG" 2>&1; then
      say "  FAILED (score) ${cand} / ${set}"
      fail=$((fail + 1))
    fi
  done
done

say ""
if [[ $fail -gt 0 ]]; then
  say "${fail} cell(s) failed; see ${LOG}"
  exit 1
fi
say "All cells completed. Reports under ${OUT_ROOT}"
