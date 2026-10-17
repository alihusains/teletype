#!/usr/bin/env bash
# P3.8 engine bake-off runner (plan §3.8).
#
# Loops engine × set × boost, calling `teletype-eval run` per cell. It is a
# PLANNER as much as a runner: with no models on disk it prints which cells
# are ready, which are disk-gated, and which are already done, then exits 0.
# It is idempotent and resumable: a cell whose output JSON already exists is
# skipped.
#
# Usage:
#   scripts/eval/run_bakeoff.sh [--models-dir <dir>] [--out-root <dir>] [--engine <id>...]
#
# The engine × set matrix lives in `teletype_eval::bakeoff` (BAKEOFF_ENGINES
# × BAKEOFF_SETS); this script mirrors it. Keep the two in sync.
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MODELS_DIR="${TELETYPE_MODELS_DIR:-}"

OUT_ROOT="${REPO_ROOT}/evals/runs/bakeoff"
EVAL_BIN=""

# Engines (catalog ids, plan §3.8 candidates).
ENGINES=(
  parakeet-ultra-coreml
  parakeet-tdt-v3
  parakeet-tdt-v2
  sherpa-parakeet-v3-int8
  sherpa-parakeet-v2-int8
  large-v3-turbo-q5
)
# Sets (plan §3.8).
SETS=(
  human
  librispeech-clean
  librispeech-other
  earnings22-vocab
  ami
  cv
  noise-10db
  noise-5db
  noise-0db
  fleurs-en
)

while [[ $# -gt 0 ]]; do
  case "$1" in
    --models-dir) MODELS_DIR="$2"; shift 2 ;;
    --out-root) OUT_ROOT="$2"; shift 2 ;;
    --eval-bin) EVAL_BIN="$2"; shift 2 ;;
    --engine)
      shift
      ENGINES=()
      while [[ $# -gt 0 && ! "$1" == --* ]]; do
        ENGINES+=("$1"); shift
      done
      ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

# Resolve the eval binary (optional: the plan works without it).
if [[ -z "$EVAL_BIN" ]]; then
  if [[ -x "${REPO_ROOT}/target/release/teletype-eval" ]]; then
    EVAL_BIN="${REPO_ROOT}/target/release/teletype-eval"
  elif command -v teletype-eval >/dev/null 2>&1; then
    EVAL_BIN="teletype-eval"
  else
    # teletype-eval links parakeet-sys whisper.cpp objects that carry no
    # __bitcode section, so the workspace release profile's LTO fails at
    # link. Build the dev tool with LTO off (it is not a shipping binary).
    echo "building teletype-eval (release, LTO off)..."
    (cd "$REPO_ROOT" && cargo build --release -p teletype-eval --config profile.release.lto=false) || exit 1
    EVAL_BIN="${REPO_ROOT}/target/release/teletype-eval"
  fi
fi

# Model file name per engine (mirrors the catalog).
model_file() {
  case "$1" in
    parakeet-tdt-v3) echo "ggml-parakeet-tdt-0.6b-v3-q8_0.bin" ;;
    parakeet-tdt-v2) echo "ggml-parakeet-tdt-0.6b-v2-q8_0.bin" ;;
    parakeet-ultra-coreml) echo "fluid" ;;
    sherpa-parakeet-v3-int8) echo "sherpa/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2" ;;
    sherpa-parakeet-v2-int8) echo "sherpa/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2" ;;
    large-v3-turbo-q5) echo "ggml-large-v3-turbo-q5_0.bin" ;;
    *) echo "" ;;
  esac
}

# Whether the model for this engine is present under MODELS_DIR.
model_present() {
  local engine="$1"
  local f
  f="$(model_file "$engine")"
  [[ -n "$MODELS_DIR" ]] || return 1
  case "$engine" in
    parakeet-ultra-coreml)
      # FluidAudio: present when the bridge can run (macOS Apple Silicon);
      # the checkpoint downloads on first load.
      [[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" ]]
      ;;
    sherpa-*)
      # sherpa model dir must carry the ONNX files.
      local dir="${MODELS_DIR}/${f}"
      [[ -f "${dir}/encoder.int8.onnx" ]]
      ;;
    *)
      [[ -f "${MODELS_DIR}/${f}" ]]
      ;;
  esac
}

declare -a READY=()
declare -a MISSING=()
declare -a DONE=()

for engine in "${ENGINES[@]}"; do
  for set in "${SETS[@]}"; do
    for boost in off on; do
      cell_out="${OUT_ROOT}/${engine}/${set}-${boost}"
      cell_json="${cell_out}/report.json"
      if [[ -f "$cell_json" ]]; then
        DONE+=("${engine}/${set}/${boost}")
        continue
      fi
      if model_present "$engine"; then
        READY+=("${engine}/${set}/${boost}")
      else
        MISSING+=("${engine}/${set}/${boost}")
      fi
    done
  done
done

echo "=== P3.8 engine bake-off plan ==="
echo "models dir: ${MODELS_DIR:-<unset>}"
echo "out root:   ${OUT_ROOT}"
echo "eval bin:   ${EVAL_BIN}"
echo
echo "ready to run:   ${#READY[@]}"
echo "already done:   ${#DONE[@]}"
echo "missing models: ${#MISSING[@]}"
echo

if [[ ${#MISSING[@]} -gt 0 ]]; then
  echo "--- missing models (disk-gated; download first) ---"
  for c in "${MISSING[@]}"; do
    echo "  ${c}"
  done
  echo
fi

if [[ ${#DONE[@]} -gt 0 ]]; then
  echo "--- already done (skipped) ---"
  for c in "${DONE[@]}"; do
    echo "  ${c}"
  done
  echo
fi

if [[ ${#READY[@]} -eq 0 ]]; then
  echo "No cells ready to run: every cell is either done or disk-gated."
  echo "Download the model weights (see evals/README.md) and re-run."
  exit 0
fi

if [[ ${#MISSING[@]} -gt 0 ]]; then
  echo "Stopping: ${#MISSING[@]} cell(s) are disk-gated (missing model weights)."
  echo "This run is a planner, not a blocker — download the weights (see"
  echo "evals/README.md) and re-run; done cells are skipped automatically."
  exit 0
fi

if [[ -z "$EVAL_BIN" ]]; then
  echo "eval binary not found; build it first: cargo build --release -p teletype-eval"
  echo "(the plan above is what will run; nothing was executed)"
  exit 0
fi
failed=0
echo "--- running ${#READY[@]} cell(s) ---"
for c in "${READY[@]}"; do
  IFS='/' read -r engine set boost <<< "$c"
  cell_out="${OUT_ROOT}/${engine}/${set}-${boost}"
  echo ">>> ${engine} / ${set} / boost=${boost}"
  if ! "$EVAL_BIN" run \
      --engine "$engine" \
      --set "$set" \
      --boost "$boost" \
      ${MODELS_DIR:+--models-dir "$MODELS_DIR"} \
      --out "$cell_out"; then
    echo "FAILED: ${c}" >&2
    failed=$((failed + 1))
  fi
done

echo
if [[ $failed -gt 0 ]]; then
  echo "${failed} cell(s) failed; re-run to retry (done cells are skipped)."
  exit 1
fi
echo "All ready cells ran. Write the decision with (point at the run dirs that exist):"
echo "  ${EVAL_BIN} compare --bake-off <evals/runs/bakeoff/<engine>/<set>-<boost>/...> \\"
echo "      --thresholds ${REPO_ROOT}/evals/thresholds.json --out ${REPO_ROOT}/docs/decisions/D-engine-defaults.md"
