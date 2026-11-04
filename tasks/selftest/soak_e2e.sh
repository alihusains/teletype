#!/usr/bin/env bash
# Headless 500-take soak: the production plan's SOAK-1 run through the app's
# --e2e take path, with no microphone, no hotkey, and no focus change.
#
# Companion to soak.sh. That leg holds the real Control key and speaks through
# the speakers, so it owns the desktop and the audio device for about three
# hours; it is still the only leg that covers the capture path. This one feeds
# WAVs straight into the armed take and, by default, forces the delivery
# cascade's hold route, so nothing is pasted and no app is brought forward.
# Run it while the machine is in use.
#
# Corpus: the app's own recordings become real-NN.wav, and the plan's other
# shapes are cut from the longest recording with ffmpeg. 16 kHz mono Int16, the
# same layout the capture path writes.
#
# Usage:
#   bash tasks/selftest/soak_e2e.sh --takes 500
#   bash tasks/selftest/soak_e2e.sh --takes 40 --out /tmp/warmup
#   bash tasks/selftest/soak_e2e.sh --gui        # paste into TextEdit instead
#                                                 # of holding; occupies focus
# Options:
#   --takes N        number of takes (default 500)
#   --model ID       speech model id (default: this machine's selection)
#   --audio DIR      recordings to sample (default: the app's recordings dir)
#   --binary PATH    app binary (default: target/debug/teletype)
#   --out DIR        output dir (default: tasks/selftest/soak-e2e-<timestamp>)
#   --gui            deliver for real instead of forcing hold
set -u
DIR="$(cd "$(dirname "$0")" && pwd)"
INTEG="${TELETYPE_INTEGRATION_DIR:-$(cd "$DIR/../.." && pwd)}"
APP_DATA="${HOME}/Library/Application Support/com.teletype.app"

TAKES=500
MODEL=""
AUDIO_IN="$APP_DATA/recordings"
BINARY="$INTEG/target/debug/teletype"
OUT=""
HEADLESS=1

while [ $# -gt 0 ]; do
  case "$1" in
    --takes) TAKES="$2"; shift 2 ;;
    --model) MODEL="$2"; shift 2 ;;
    --audio) AUDIO_IN="$2"; shift 2 ;;
    --binary) BINARY="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --gui) HEADLESS=0; shift ;;
    *) echo "unknown option: $1" >&2; exit 1 ;;
  esac
done

[ -x "$BINARY" ] || { echo "no app binary at $BINARY (build it first)"; exit 1; }
[ -d "$AUDIO_IN" ] || { echo "no recordings dir at $AUDIO_IN"; exit 1; }
command -v ffmpeg >/dev/null || { echo "ffmpeg is needed to cut the shape clips"; exit 1; }
command -v python3 >/dev/null || { echo "python3 is needed for the driver"; exit 1; }

[ -n "$OUT" ] || OUT="$DIR/soak-e2e-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$OUT/config" "$OUT/data" "$OUT/audio"
ln -sfn "$APP_DATA/models" "$OUT/data/models"

# ---- corpus ---------------------------------------------------------------
# Real takes, capped so a cycle rotates through distinct speech. Quoted glob:
# the app's data dir lives under "Application Support", so these paths always
# contain a space.
n=0
for f in "$AUDIO_IN"/*.wav; do
  [ -e "$f" ] || continue
  n=$((n + 1))
  [ "$n" -le 8 ] || break
  cp "$f" "$OUT/audio/$(printf 'real-%02d.wav' "$n")"
done
[ "$n" -gt 0 ] || { echo "no WAV files in $AUDIO_IN"; exit 1; }

# The base for the synthetic shapes: the largest real clip, so looping it to
# five minutes repeats speech rather than silence.
BASE=$(ls -S "$OUT"/audio/real-*.wav | head -1)
cut16() { # cut16 <out> <ffmpeg input args...>
  local out="$1"; shift
  ffmpeg -hide_banner -loglevel error -y "$@" -ar 16000 -ac 1 -c:a pcm_s16le \
    "$OUT/audio/$out" || echo "warning: could not make $out"
}
cut16 sil_3s.wav     -f lavfi -i anullsrc=r=16000:cl=mono -t 3
cut16 sil_15s.wav    -f lavfi -i anullsrc=r=16000:cl=mono -t 15
cut16 tap_0s2.wav    -i "$BASE" -t 0.2
cut16 speech_2s.wav  -i "$BASE" -t 2
cut16 speech_30s.wav -stream_loop -1 -i "$BASE" -t 30
cut16 speech_5min.wav -stream_loop -1 -i "$BASE" -t 300

# ---- settings -------------------------------------------------------------
# A copy of this machine's settings with the three switches that would change
# what a soak means turned off: command mode (it would act on dictated
# commands), saving recordings (it would grow the corpus mid-run), and the
# Notetaker route (it would take the dictation out of the delivery cascade).
MODEL=${MODEL:-$(python3 -c "
import json;print(json.load(open('$APP_DATA/settings.json'))
                 .get('selectedSpeechModel','parakeet-tdt-v3'))" 2>/dev/null \
  || echo parakeet-tdt-v3)}
python3 - "$APP_DATA/settings.json" "$OUT/config/settings.json" "$MODEL" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
d["selectedSpeechModel"] = sys.argv[3]
d["commandModeEnabled"] = False
d["saveRecordings"] = False
d["scratchpadEnabled"] = False
json.dump(d, open(sys.argv[2], "w"), indent=2)
PY

# ---- run ------------------------------------------------------------------
SOCK="$OUT.sock"
LOG="$OUT/app.log"
rm -f "$SOCK" "$LOG"

FORCE=""
if [ "$HEADLESS" = "1" ]; then
  # The cascade returns HeldForPasteLast before any clipboard write or
  # keystroke, so no app needs focus for the whole run.
  FORCE="TELETYPE_FORCE_DELIVERY=hold"
else
  printf 'soak target\n' > "$OUT/target.txt"
  open -a TextEdit "$OUT/target.txt" >/dev/null 2>&1
  osascript -e 'tell application "TextEdit" to activate' >/dev/null 2>&1
fi
sleep 1

cd "$INTEG" || exit 1
env TELETYPE_E2E_SOCKET="$SOCK" TELETYPE_E2E_CONFIG_DIR="$OUT/config" \
  TELETYPE_E2E_DATA_DIR="$OUT/data" $FORCE \
  "$BINARY" --e2e > "$LOG" 2>&1 &
APP=$!
echo "soak_e2e: app pid=$APP model=$MODEL takes=$TAKES headless=$HEADLESS out=$OUT"
for _ in $(seq 1 60); do [ -S "$SOCK" ] && break; sleep 0.5; done
if [ ! -S "$SOCK" ]; then
  echo "the app did not open its e2e socket; last log lines:"
  tail -20 "$LOG"
  kill -9 "$APP" 2>/dev/null
  exit 1
fi
sleep 5

SOAK_SOCK="$SOCK" SOAK_APP_PID="$APP" SOAK_DATA_DIR="$OUT/data" SOAK_LOG="$LOG" \
SOAK_OUT="$OUT" SOAK_AUDIO="$OUT/audio" SOAK_TAKES="$TAKES" SOAK_HEADLESS="$HEADLESS" \
  python3 "$DIR/soak_e2e.py"
DRIVER=$?

kill -9 "$APP" 2>/dev/null
# A forced kill mid-take is the crash-recovery path, not a soak result: say so
# rather than letting the leftover spool read as a finding.
echo "results: $OUT/takes.jsonl and $OUT/summary.json"
exit $DRIVER
