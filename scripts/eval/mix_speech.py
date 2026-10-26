#!/usr/bin/env python3
"""Mix a second speaker into a clean clip at a target speech-to-speech ratio.

This is the two-speaker analogue of `mix_noise.py`: it exists because the
corpus has no set in which a *human* competes with the target talker. Every
"noise" tier is LibriSpeech speech plus DEMAND/MUSAN noise, which is a
different axis: a stationary bed is easier for a TDT transducer than an
informational masker with the same energy, because the interferer's own
phonemes compete with the target's.

Stdlib only (no numpy), same contract as `mix_noise.py`:

  * reads 16-bit PCM mono WAVs at the same rate;
  * loops the interferer to cover the target, starting at a seeded offset;
  * delays the interferer by `--delay-ms` so the two talkers do not begin on
    the same sample (real overlap rarely starts dead-on);
  * scales the interferer so the ratio of **speech-active** levels equals
    `--snr-db`, using the same convention as the `noise-{snr}db` sets: 0 dB is
    "the other talker is as loud as the person dictating", negative dB means
    the interferer is *louder*, positive dB means it is quieter;
  * if the sum would clip, scales the whole mix to peak 0.98 and says so.

Whole-file RMS is the wrong level metric here and was the first bug in this
tool: LibriSpeech clips carry very different amounts of leading/trailing
silence, so a naturally gappy interferer got boosted until its *speech* was
several dB above the target's. `speech_level` measures the frames that
actually carry speech (within 25 dB of the loudest frame, and never below
-50 dBFS), so 0 dB means the two voices are equally loud where it matters.

The applied gain and both level ratios are written to `<out>.json` so a
caller can audit the mix instead of trusting the print.
"""

from __future__ import annotations

import argparse
import json
import math
import random
import struct
import sys
import wave


def die(msg: str) -> "NoReturn":
    print(f"mix_speech.py: error: {msg}", file=sys.stderr)
    sys.exit(1)


def read_mono_wav(path: str) -> tuple[list[float], int]:
    try:
        with wave.open(path, "rb") as w:
            nch, width, rate, nframes = (
                w.getnchannels(),
                w.getsampwidth(),
                w.getframerate(),
                w.getnframes(),
            )
            raw = w.readframes(nframes)
    except wave.Error as e:
        die(f"cannot read {path}: {e}")
    if width != 2:
        die(f"{path}: only 16-bit PCM is supported (got {width * 8}-bit)")
    n = len(raw) // (2 * nch)
    samples = []
    for i in range(n):
        acc = 0.0
        for c in range(nch):
            (v,) = struct.unpack_from("<h", raw, (i * nch + c) * 2)
            acc += v / 32768.0
        samples.append(acc / nch)
    return samples, rate


def rms(x: list[float]) -> float:
    if not x:
        return 0.0
    return math.sqrt(sum(v * v for v in x) / len(x))


def frame_powers(x: list[float], rate: int, win_ms: float = 25.0, hop_ms: float = 10.0) -> list[float]:
    """Mean power of each overlapping window, in linear units."""
    win = max(1, int(rate * win_ms / 1000.0))
    hop = max(1, int(rate * hop_ms / 1000.0))
    powers = []
    for start in range(0, max(1, len(x) - win + 1), hop):
        seg = x[start:start + win]
        if not seg:
            break
        powers.append(sum(v * v for v in seg) / len(seg))
    return powers


def speech_level(x: list[float], rate: int) -> float:
    """RMS amplitude over the frames that carry speech.

    A frame counts as speech when it is within `SPEECH_RANGE_DB` of the
    loudest frame and above the absolute floor. Averaging power over those
    frames ignores the pauses that make whole-file RMS depend on editing
    rather than on loudness.
    """
    powers = [p for p in frame_powers(x, rate) if p > 0.0]
    if not powers:
        return 0.0
    loudest_db = 10.0 * math.log10(max(powers))
    floor_db = max(loudest_db - SPEECH_RANGE_DB, ABS_FLOOR_DB)
    active = [p for p in powers if 10.0 * math.log10(p) >= floor_db]
    return math.sqrt(sum(active) / len(active))


SPEECH_RANGE_DB = 25.0
ABS_FLOOR_DB = -50.0


def write_mono_wav(path: str, samples: list[float], rate: int) -> float:
    """Write the mix; returns the linear scale applied to avoid clipping."""
    peak = max([abs(v) for v in samples], default=0.0)
    scale = 1.0
    if peak > 1.0:
        scale = 0.98 / peak
        print(f"note: mix clips at peak {peak:.2f}; scaling by {scale:.3f}", file=sys.stderr)
        samples = [v * scale for v in samples]
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(struct.pack(f"<{len(samples)}h", *[int(max(-1.0, min(1.0, v)) * 32767) for v in samples]))
    return scale


def loop_to(voice: list[float], n: int, offset: int) -> list[float]:
    if not voice:
        die("interferer file has no samples")
    return [voice[(offset + i) % len(voice)] for i in range(n)]


def self_test() -> None:
    """Validate the level metric: a half-amplitude tone must read 6.02 dB down."""
    rate = 16000
    tone = [0.2 * math.sin(2 * math.pi * 1000 * i / rate) for i in range(rate)]
    quiet = [v / 2.0 for v in tone]
    got = 20.0 * math.log10(speech_level(tone, rate) / speech_level(quiet, rate))
    if abs(got - 6.02) > 0.05:
        die(f"self-test failed: amplitude halving measured {got:.2f} dB, expected 6.02")
    print(f"self-test ok: amplitude halving -> {got:.2f} dB")


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--target", help="clip to be transcribed (WAV)")
    p.add_argument("--interferer", help="second talker's WAV (looped to fit)")
    p.add_argument("--snr-db", type=float,
                   help="interferer level relative to the target, dB "
                        "(0 = equal loudness, -5 = interferer 5 dB louder)")
    p.add_argument("--delay-ms", type=int, default=300,
                   help="interferer starts this many ms after the target (default 300)")
    p.add_argument("--out", help="output mixed WAV")
    p.add_argument("--seed", type=int, default=0, help="loop-offset seed")
    p.add_argument("--self-test", action="store_true", help="check the level metric and exit")
    args = p.parse_args()
    if args.self_test:
        self_test()
        return
    for required in ("target", "interferer", "snr_db", "out"):
        if getattr(args, required) is None:
            die(f"--{required.replace('_', '-')} is required")

    target, target_rate = read_mono_wav(args.target)
    other, other_rate = read_mono_wav(args.interferer)
    if other_rate != target_rate:
        die(f"rate mismatch: target {target_rate} Hz vs interferer {other_rate} Hz "
            "(convert both to 16 kHz first)")
    target_level = speech_level(target, target_rate)
    other_level = speech_level(other, other_rate)
    if target_level == 0.0:
        die(f"{args.target}: target file is silent; the ratio is undefined")
    if other_level == 0.0:
        die(f"{args.interferer}: interferer file is silent")

    rng = random.Random(args.seed)
    lead = max(0, int(args.delay_ms * target_rate / 1000))
    spoken = max(0, len(target) - lead)
    bed = loop_to(other, spoken, rng.randrange(len(other)))
    # Measure the segment that is actually mixed, not the whole interferer
    # file: a looped or truncated tail can be quieter or louder than the file
    # as a whole, and using the file's level put the realised ratio up to
    # 1.5 dB off the request.
    bed_level = speech_level(bed, target_rate)
    if bed_level == 0.0:
        die(f"{args.interferer}: the mixed segment is silent after looping")
    target_ratio = 10.0 ** (args.snr_db / 20.0)
    gain = target_level / bed_level / target_ratio

    mixed = list(target)
    for i, v in enumerate(bed):
        mixed[lead + i] += gain * v
    scale = write_mono_wav(args.out, mixed, target_rate)

    measured = 20.0 * math.log10(target_level / (gain * bed_level))
    rms_ratio = 20.0 * math.log10(rms(target) / (gain * rms(bed) or 1e-9))
    sidecar = {
        "target": args.target,
        "interferer": args.interferer,
        "requested_snr_db": args.snr_db,
        "measured_speech_ratio_db": round(measured, 3),
        "measured_rms_ratio_db": round(rms_ratio, 3),
        "target_speech_level_dbfs": round(20.0 * math.log10(target_level), 2),
        "interferer_speech_level_dbfs": round(20.0 * math.log10(other_level), 2),
        "gain_db": round(20.0 * math.log10(gain), 3),
        "delay_ms": args.delay_ms,
        "looped": len(other) < spoken,
        "clip_scale": round(scale, 4),
    }
    with open(args.out + ".json", "w") as f:
        json.dump(sidecar, f)
        f.write("\n")
    print(f"mixed {args.target} + {args.interferer} at {measured:.2f} dB "
          f"(delay {args.delay_ms} ms, gain {sidecar['gain_db']:+.2f} dB) -> {args.out}")


if __name__ == "__main__":
    main()
