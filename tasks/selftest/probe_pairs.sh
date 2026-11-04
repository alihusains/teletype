#!/usr/bin/env bash
# Run the take-pair probe over the sequences that can settle the SOAK-1
# quiet-end question, against one app instance so the pairs share a session.
#
# Each configuration arms first, then second, N times, and reports how often
# the SECOND take ends quietly. The control is the pair where the first take
# cannot leave a trailing release (speech into speech); if a short first take
# raises the quiet rate on the second, the harness is cutting takes short
# rather than the app losing a user's speech.
#
# Headless: TELETYPE_FORCE_DELIVERY=hold, so nothing is typed and the
# clipboard is left alone for the whole run.
set -u

INTEG=/Users/a.sorathiya/Documents/Ali/teletype-wt/integration
BINARY="$INTEG/target/debug/teletype"
SRC=/tmp/soak500b
OUT="${OUT:-/tmp/pairprobe}"
REPS="${REPS:-6}"

rm -rf "$OUT"
mkdir -p "$OUT/config" "$OUT/data"
SOCK="$OUT.sock"
LOG="$OUT/app.log"

# A profile that already has the speech model, so a gated press cannot be
# mistaken for a quiet end. clonefile makes the model copy instant on APFS.
cp "$SRC/config/settings.json" "$OUT/config/settings.json"
if ! cp -c -R "$SRC/data/models" "$OUT/data/models" 2>/dev/null; then
  cp -R "$SRC/data/models" "$OUT/data/models"
fi
echo "pairprobe: profile at $OUT, models cloned, reps=$REPS"

env TELETYPE_E2E_SOCKET="$SOCK" TELETYPE_E2E_CONFIG_DIR="$OUT/config" \
  TELETYPE_E2E_DATA_DIR="$OUT/data" TELETYPE_FORCE_DELIVERY=hold \
  "$BINARY" --e2e > "$LOG" 2>&1 &
APP=$!
for _ in $(seq 1 120); do [ -S "$SOCK" ] && break; sleep 0.5; done
if [ ! -S "$SOCK" ]; then
  echo "pairprobe: the app did not open its e2e socket; last log lines:"
  tail -20 "$LOG"
  kill -9 "$APP" 2>/dev/null
  exit 1
fi
sleep 5

cd "$INTEG" || exit 1
run() {
  local first="$1" second="$2" gap="$3" settle="$4" tag="$5"
  echo
  echo "--- $tag: first=$first second=$second gap=${gap}s settle=$settle ---"
  if [ "$settle" = "on" ]; then
    python3 tasks/selftest/take_pair_probe.py --sock "$SOCK" --log "$LOG" \
      --audio "$SRC/audio" --first "$first" --second "$second" \
      --reps "$REPS" --gap "$gap" --out "$OUT/$tag" 2>&1 | tail -8
  else
    python3 tasks/selftest/take_pair_probe.py --sock "$SOCK" --log "$LOG" \
      --audio "$SRC/audio" --first "$first" --second "$second" \
      --reps "$REPS" --gap "$gap" --no-settle --out "$OUT/$tag" 2>&1 | tail -8
  fi
}

# The control: the same shape on both sides. The negative control is the same
# pair with the gates on, which is also the configuration the soak driver
# uses. The suspects: a short tap, and a spam burst, each followed by a speech
# take, with no settle gate and no gap (the artifact-prone case) and then with
# the gates and an 8 s gap (the guarded case).
run tap-short tap-short 0 off control-taptap-nosettle
run speech-real speech-real 8 on control-speech-speech-settled
run tap-short speech-real 0 off tap-then-speech-nosettle
run tap-short speech-real 8 on tap-then-speech-settled
run spam speech-real 0 off spam-then-speech-nosettle
run spam speech-real 8 on spam-then-speech-settled

echo
echo "=== summaries ==="
for d in "$OUT"/*/; do
  [ -f "$d/summary.json" ] && python3 -c "
import json,sys
s=json.load(open('$d/summary.json'))
print('%-34s second take quiet %s/%s  gap=%s settle=%s' % (
  s['pair']['first']+'->'+s['pair']['second'],
  s['second']['quiet_ends'], s['second']['reps'],
  s['gap_seconds'], s['settle_gates']))
"
done
kill -9 "$APP" 2>/dev/null
echo "pairprobe: per-rep records under $OUT/<tag>/takes.jsonl"
