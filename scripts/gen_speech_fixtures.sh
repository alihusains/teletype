#!/usr/bin/env bash
# Generates the teletype-speech conformance WAV fixtures (16 kHz mono s16le).
#
#   crates/teletype-speech/tests/fixtures/
#     hello.wav                 ~1 s, "hello"
#     names.wav                 "Siobhan O'Rourke, Aoife, PyTorch, Kubernetes"
#     numbers.wav               "forty two, three point one four, nineteen eighty"
#     long_60s.wav              >= 55 s of speech (verified with afinfo)
#     noise_0db.wav             hello.wav mixed with uniform noise at 0 dB SNR
#     silence.wav               ~2 s of zeros
#     first_word_immediate.wav  0.2 s silence + 0.15 s speech
#
# Re-runnable and idempotent: regenerates every fixture into the fixtures dir.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$REPO/crates/teletype-speech/tests/fixtures"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

for tool in say afconvert afinfo python3; do
  command -v "$tool" >/dev/null 2>&1 || { echo "missing required tool: $tool" >&2; exit 1; }
done

mkdir -p "$OUT"

say_to_wav() { # <out.wav> <text>
  local tmp_aiff="$TMP/$(basename "$1" .wav).aiff"
  say -o "$tmp_aiff" "$2"
  afconvert -f WAVE -d LEI16@16000 -c 1 "$tmp_aiff" "$1"
}

# --- hello.wav ----------------------------------------------------------
say_to_wav "$OUT/hello.wav" "hello"

# --- names.wav ----------------------------------------------------------
say_to_wav "$OUT/names.wav" "Siobhan O'Rourke, Aoife, PyTorch, Kubernetes"

# --- numbers.wav --------------------------------------------------------
say_to_wav "$OUT/numbers.wav" "forty two, three point one four, nineteen eighty"

# --- long_60s.wav (>= 55 s) ---------------------------------------------
# Loop the hello clip until it covers 58 s, then trim to exactly 58 s.
python3 - "$OUT/hello.wav" "$OUT/long_60s.wav" <<'EOF'
import struct, sys, wave

src, dst = sys.argv[1], sys.argv[2]
RATE = 16000
TARGET = 58.0

with wave.open(src, "rb") as w:
    assert w.getnchannels() == 1 and w.getsampwidth() == 2 and w.getframerate() == RATE
    chunk = w.readframes(w.getnframes())

needed = int(TARGET * RATE) * 2
data = (chunk * (needed // len(chunk) + 1))[:needed]
with wave.open(dst, "wb") as w:
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(RATE)
    w.writeframes(data)
EOF

DUR="$(afinfo "$OUT/long_60s.wav" | awk -F': ' '/duration/{print $2; exit}')"
DUR_SECS="$(printf '%s' "$DUR" | awk -F. '{print $1}')"
if [ "${DUR_SECS:-0}" -lt 55 ]; then
  echo "long_60s.wav too short: $DUR" >&2
  exit 1
fi

# --- silence.wav (~2 s of zeros) ----------------------------------------
python3 - "$OUT/silence.wav" <<'EOF'
import sys, wave
with wave.open(sys.argv[1], "wb") as w:
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(16000)
    w.writeframes(b"\x00\x00" * (2 * 16000))
EOF

# --- noise_0db.wav (hello + uniform noise at 0 dB SNR, deterministic) ---
python3 - "$OUT/hello.wav" "$OUT/noise_0db.wav" <<'EOF'
import random, struct, sys, wave

src, dst = sys.argv[1], sys.argv[2]
RATE = 16000

with wave.open(src, "rb") as w:
    assert w.getnchannels() == 1 and w.getsampwidth() == 2
    frames = w.readframes(w.getnframes())
n = len(frames) // 2
sig = list(struct.unpack("<%dh" % n, frames))
sig_power = sum(s * s for s in sig) / max(n, 1)

rng = random.Random(1337)
noise = [int(rng.uniform(-1, 1) * 32767) for _ in range(n)]
noise_power = sum(x * x for x in noise) / max(n, 1)

# 0 dB SNR: scale the noise channel to match the signal's power.
if noise_power > 0:
    scale = (sig_power / noise_power) ** 0.5
    noise = [max(-32768, min(32767, int(x * scale))) for x in noise]

mixed = [max(-32768, min(32767, s + x)) for s, x in zip(sig, noise)]
with wave.open(dst, "wb") as w:
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(RATE)
    w.writeframes(struct.pack("<%dh" % n, *mixed))
EOF

# --- first_word_immediate.wav (0.2 s silence + 0.15 s speech) -----------
# Trim the first 0.15 s of hello.wav, prepend 0.2 s of silence.
python3 - "$OUT/hello.wav" "$OUT/first_word_immediate.wav" <<'EOF'
import struct, sys, wave

src, dst = sys.argv[1], sys.argv[2]
RATE = 16000

with wave.open(src, "rb") as w:
    assert w.getnchannels() == 1 and w.getsampwidth() == 2 and w.getframerate() == RATE
    frames = w.readframes(w.getnframes())

speech = frames[: int(0.15 * RATE * 2)]
silence = b"\x00\x00" * int(0.2 * RATE)
with wave.open(dst, "wb") as w:
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(RATE)
    w.writeframes(silence + speech)
EOF

echo "fixtures written to $OUT"
