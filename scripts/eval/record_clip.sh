#!/usr/bin/env bash
# Record one human-corpus clip in the plan §P0.4 four-file format:
#   <id>.wav          16-bit mono WAV (any rate; the harness resamples)
#   <id>.verbatim.txt what was actually said, fillers included
#   <id>.clean.txt    what the user wants pasted
#   <id>.meta.json    {speaker, mic, noise, app_context, vocab:[...]}
#
# Usage: scripts/eval/record_clip.sh <clip-id> [out-dir]
#   clip-id: [a-z0-9-] (e.g. `js-014`), out-dir defaults to evals/human-corpus.
#
# Recording backend, first available wins: sox (rec), ffmpeg (avfoundation on
# macOS, alsa/pulse on Linux), afrecord (macOS). Press Ctrl+C to stop.
# This script never records anything by itself — a human speaks into the mic.
set -euo pipefail

ID="${1:-}"
OUT="${2:-evals/human-corpus}"

if [[ ! "$ID" =~ ^[a-z0-9-]+$ ]]; then
  echo "usage: record_clip.sh <clip-id[a-z0-9-]> [out-dir]" >&2
  exit 1
fi
mkdir -p "$OUT"
WAV="$OUT/$ID.wav"
if [[ -e "$WAV" && "${FORCE:-}" != "1" ]]; then
  echo "$WAV exists; set FORCE=1 to overwrite" >&2
  exit 1
fi

record() {
  if command -v sox >/dev/null 2>&1; then
    echo "recording with sox — speak now, Ctrl+C to stop"
    rec -c 1 -b 16 "$WAV" gain -n
  elif command -v ffmpeg >/dev/null 2>&1; then
    case "$(uname -s)" in
      Darwin)
        echo "recording with ffmpeg/avfoundation — speak now, Ctrl+C (or 'q') to stop"
        ffmpeg -y -v error -f avfoundation -i ":0" -ac 1 -ar 16000 -c:a pcm_s16le "$WAV"
        ;;
      Linux)
        echo "recording with ffmpeg/alsa — speak now, Ctrl+C (or 'q') to stop"
        ffmpeg -y -v error -f alsa -i default -ac 1 -ar 16000 -c:a pcm_s16le "$WAV"
        ;;
      *)
        echo "unsupported OS for ffmpeg capture: $(uname -s)" >&2; return 1
        ;;
    esac
  elif [[ "$(uname -s)" == "Darwin" ]] && command -v afrecord >/dev/null 2>&1; then
    echo "recording with afrecord — speak now, Ctrl+C to stop"
    afrecord "$WAV" -d 0.1 -f WAVE -d LEI16@16000 -c 1
  else
    echo "no recorder found (need one of: sox, ffmpeg, afrecord)" >&2
    return 1
  fi
}
record

echo "saved $WAV"
echo "Now type what you SAID (verbatim, fillers included), one line, then Enter:"
IFS= read -r VERBATIM < /dev/tty
echo "Now type what should be PASTED (clean), one line, then Enter:"
IFS= read -r CLEAN < /dev/tty
printf '%s\n' "$VERBATIM" > "$OUT/$ID.verbatim.txt"
printf '%s\n' "$CLEAN" > "$OUT/$ID.clean.txt"

echo "Speaker tag (e.g. initials; no real names needed):"
IFS= read -r SPEAKER < /dev/tty
echo "Mic [macbook/airpods/usb/other]:"
IFS= read -r MIC < /dev/tty
echo "Noise [quiet/cafe/street/other]:"
IFS= read -r NOISE < /dev/tty
echo "App context [email/chat/code/notes/other]:"
IFS= read -r CTX < /dev/tty
echo "Vocab terms, comma-separated (names, tickers, jargon — or empty):"
IFS= read -r VOCAB < /dev/tty

python3 - "$OUT/$ID.meta.json" "$SPEAKER" "$MIC" "$NOISE" "$CTX" "$VOCAB" <<'EOF'
import json, sys
path, speaker, mic, noise, ctx, vocab = sys.argv[1:7]
terms = [t.strip() for t in vocab.split(",") if t.strip()]
meta = {"speaker": speaker.strip(), "mic": mic.strip(), "noise": noise.strip(),
        "app_context": ctx.strip(), "vocab": terms}
json.dump(meta, open(path, "w"), indent=2)
print("wrote", path)
EOF
echo "done: $ID.{wav,verbatim.txt,clean.txt,meta.json}"
