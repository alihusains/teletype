#!/usr/bin/env bash
# Download the plan §P0.4 public eval sets into the dataset cache.
#
# Usage:
#   scripts/eval/fetch_datasets.sh [set ...]
#
# With no arguments, fetches every set. With arguments, fetches only those
# (names: librispeech-clean, librispeech-other, earnings22-vocab, ami, cv,
#  fleurs, noise, musan). Layout (never committed; add the cache dir to your
#  global gitignore — it lives outside the repo):
#
#   ${TELETYPE_EVAL_CACHE:-~/.cache/teletype-eval}/
#     librispeech-clean/manifest.jsonl + audio/
#     librispeech-other/...
#     earnings22-vocab/... + vocab.json
#     ami/...  cv/...  fleurs-<lang>/...  noise-{10,5,0}db/...  musan/ (raw)
#
# Properties: idempotent (a finished set dir with `.done` is skipped),
# resumable (`curl -C -` for archives, streaming HF rows for the rest).
# Public sources only (OpenSLR, Hugging Face). Needs: bash, curl, tar,
# python3, ffmpeg. Hugging Face sets additionally need `pip install datasets`.
#
# Heavy downloads (GBs): LibriSpeech ~1 GB, MUSAN ~2 GB, CV/FLEURS stream a
# few hundred MB each. Time-box to taste — pass set names to fetch a subset.
set -euo pipefail

CACHE="${TELETYPE_EVAL_CACHE:-$HOME/.cache/teletype-eval}"
SEED="${EVAL_SEED:-20261014}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OPENSLR="https://www.openslr.org/resources"

need() { command -v "$1" >/dev/null 2>&1 || { echo "fetch_datasets.sh: need '$1' on PATH" >&2; exit 1; }; }
need curl; need tar; need python3; need ffmpeg

mkdir -p "$CACHE"

fetch_archive() { # <url> <dest-file>
  local url="$1" dest="$2"
  if [[ -f "$dest" ]]; then echo "have $(basename "$dest")"; return 0; fi
  echo "downloading $url"
  curl -fSL --retry 3 -C - -o "$dest" "$url"
}

want() { # <set> <all...>: true when no filter or set is listed
  local needle="$1"; shift
  [[ $# -eq 0 ]] && return 0
  local s; for s in "$@"; do [[ "$s" == "$needle" ]] && return 0; done
  return 1
}

# ── LibriSpeech test-clean / test-other (OpenSLR 12) ─────────────────────
# 200 utterances each, fixed seed. Transcripts ship inside the archives.
librispeech_split() { # <split: test-clean|test-other> <set-name> <n>
  local split="$1" set="$2" n="$3"
  local dir="$CACHE/$set"
  [[ -f "$dir/.done" ]] && { echo "have $set"; return 0; }
  local tgz="$CACHE/${split}.tar.gz"
  fetch_archive "$OPENSLR/12/${split}.tar.gz" "$tgz"
  local raw="$CACHE/raw-librispeech"
  mkdir -p "$raw"
  tar -xzf "$tgz" -C "$raw"
  # Flatten <spk>/<ch>/*.trans.txt into audio+reference JSONL.
  python3 - "$raw/LibriSpeech/$split" "$CACHE/${set}.jsonl" <<'EOF'
import glob, json, os, sys
root, out = sys.argv[1], sys.argv[2]
with open(out, "w") as f:
    for trans in sorted(glob.glob(os.path.join(root, "*", "*", "*.trans.txt"))):
        d = os.path.dirname(trans)
        for line in open(trans):
            utt, _, text = line.strip().partition(" ")
            flac = os.path.join(d, utt + ".flac")
            if os.path.exists(flac):
                f.write(json.dumps({"audio": flac, "reference": text, "tags": ["librispeech"]}) + "\n")
print("rows:", sum(1 for _ in open(out)))
EOF
  # shellcheck disable=SC2086
  python3 "$SCRIPT_DIR/subset.py" sample --from-jsonl "$CACHE/${set}.jsonl" \
    --out "$dir" --set "$set" --n "$n" --seed "$SEED"
  date -u +%FT%TZ > "$dir/.done"
}

# ── MUSAN (OpenSLR 17) + noise mixes ─────────────────────────────────────
# Mixes at 10/5/0 dB over 100 test-other clips (plan §P0.4). Noise files are
# chosen by a seeded RNG, so re-runs reproduce the same mixes.
musan_set() {
  [[ -f "$CACHE/musan/.done" ]] || {
    local tgz="$CACHE/musan.tar.gz"
    fetch_archive "$OPENSLR/17/musan.tar.gz" "$tgz"
    tar -xzf "$tgz" -C "$CACHE"
    date -u +%FT%TZ > "$CACHE/musan/.done"
  }
  echo "have musan raw"
  local clean="$CACHE/librispeech-other"
  [[ -f "$clean/.done" ]] || { echo "noise sets need librispeech-other first" >&2; return 1; }
  for db in 10 5 0; do
    local set="noise-${db}db"
    local out="$CACHE/$set"
    [[ -f "$out/.done" ]] && { echo "have $set"; continue; }
    echo "building $set"
    MUSAN_CACHE="$CACHE" MIXER="$SCRIPT_DIR/mix_noise.py" \
    python3 - "$clean/manifest.jsonl" "$out" "$db" "$SEED" <<'EOF'
import glob, json, os, random, subprocess, sys
manifest, out_dir, db, seed = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
cache = os.environ["MUSAN_CACHE"]
mixer = os.environ["MIXER"]
musan = sorted(glob.glob(os.path.join(cache, "musan", "*", "*", "*.wav")))
assert musan, "no musan wavs under musan/*/*/*.wav"
rows = [json.loads(l) for l in open(manifest) if l.strip()]
rng = random.Random(seed)
picked = sorted(rng.sample(rows, min(100, len(rows))), key=lambda r: r["id"])
os.makedirs(os.path.join(out_dir, "audio"), exist_ok=True)
manifest_dir = os.path.dirname(os.path.abspath(manifest))
with open(os.path.join(out_dir, "manifest.jsonl"), "w") as f:
    for i, row in enumerate(picked):
        cid = f"noise-{db}db-{i+1:04d}"
        clean = row["audio"]
        if not os.path.isabs(clean):
            clean = os.path.join(manifest_dir, clean)
        dest = os.path.join(out_dir, "audio", cid + ".wav")
        subprocess.run([sys.executable, mixer, "--clean", clean,
                        "--noise", rng.choice(musan), "--snr-db", db,
                        "--out", dest, "--seed", str(seed + i)], check=True)
        f.write(json.dumps({"id": cid, "audio": f"audio/{cid}.wav",
                            "reference": row["reference"],
                            "tags": ["noise", f"snr-{db}db"], "human": False}) + "\n")
print("mixed", len(picked), "clips at", db, "dB")
EOF
    date -u +%FT%TZ > "$out/.done"
  done
}

# ── Hugging Face sets (streamed, resumable by re-run) ────────────────────
hf_set() { # <hf-id> <config> <split> <audio-col> <text-col> <set> <n>
  local hf="$1" config="$2" split="$3" acol="$4" tcol="$5" set="$6" n="$7"
  local dir="$CACHE/$set"
  [[ -f "$dir/.done" ]] && { echo "have $set"; return 0; }
  python3 -c "import datasets" 2>/dev/null || {
    echo "skip $set: 'pip install datasets' first" >&2; return 1
  }
  # shellcheck disable=SC2086
  python3 "$SCRIPT_DIR/subset.py" hf --dataset "$hf" \
    ${config:+--config "$config"} --split "$split" \
    --audio-col "$acol" --text-col "$tcol" \
    --out "$dir" --set "$set" --n "$n" --seed "$SEED"
  date -u +%FT%TZ > "$dir/.done"
}

main() {
  export TELETYPE_MIX_NOISE_PY="$SCRIPT_DIR/mix_noise.py"
  want librispeech-clean "$@" && librispeech_split test-clean librispeech-clean 200
  want librispeech-other "$@" && librispeech_split test-other librispeech-other 200
  want earnings22-vocab "$@" && {
    # The -chunked repo ships chunk audio but no per-chunk transcripts
    # (references live in the parent Earnings22-Cleaned-AA transcripts), so a
    # dedicated builder apportions parent words to chunks, samples 100
    # segments, and writes manifest.jsonl + vocab.json.
    python3 "$SCRIPT_DIR/build_earnings22_segments.py" "$CACHE/earnings22-vocab"
  }
  # AMI IHM: no reliable public mirror exists on the Hub (checked 2026-10-14:
  # only an unofficial metadata-only copy). Try the community mirror, then
  # fall back to documented manual steps (official corpus needs a request at
  # https://groups.inf.ed.ac.uk/ami/download/).
  want ami "$@" && {
    hf_set leoapolonio/AMI_Meeting_Corpus "" train audio transcript ami 100 || {
      echo "AMI: automatic fetch failed; manual path: request the AMI Meeting Corpus" >&2
      echo "at https://groups.inf.ed.ac.uk/ami/download/, convert 100 IHM segments to" >&2
      echo "16 kHz mono WAV, and lay them out as \$CACHE/ami/{manifest.jsonl,audio/}." >&2
    }
  }
  want cv "$@" && hf_set mozilla-foundation/common_voice_17_0 en test audio sentence cv 200
  if want fleurs "$@"; then
    for lang in en de fr es it pt nl pl ru uk; do
      hf_set google/fleurs "$lang" test audio transcription "fleurs-$lang" 50 || true
    done
  fi
  { want noise "$@" || want musan "$@"; } && musan_set
  echo "cache: $CACHE"; ls "$CACHE"
}

main "$@"
