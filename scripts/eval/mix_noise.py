#!/usr/bin/env python3
"""Mix MUSAN (or any) noise into a clean clip at a target SNR.

Stdlib only (no numpy): reads 16-bit PCM WAVs, loops the noise to cover the
clean clip, scales it to the requested signal-to-noise ratio, and writes the
mix. Used by fetch_datasets.sh to build the `noise-{10,5,0,-5}db` eval sets
(plan §5.8 adds the -5 dB tier).

SNR is computed from whole-file RMS. If the mix would clip, it is scaled to
peak 0.98 and the applied extra attenuation is reported (the effective SNR
is then higher than requested — the message says so; eval sets should check
for it).

Negative SNR (noise louder than speech) is supported: the noise bed is
scaled up relative to the clean signal. The mixer still refuses to let the
clean file be silent, because SNR is undefined there.
"""

from __future__ import annotations

import argparse
import math
import random
import struct
import sys
import wave


def die(msg: str) -> "NoReturn":
    print(f"mix_noise.py: error: {msg}", file=sys.stderr)
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


def write_mono_wav(path: str, samples: list[float], rate: int) -> None:
    peak = max([abs(v) for v in samples], default=0.0)
    if peak > 1.0:
        scale = 0.98 / peak
        print(f"note: mix clips at peak {peak:.2f}; scaling by {scale:.3f}", file=sys.stderr)
        samples = [v * scale for v in samples]
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(struct.pack(f"<{len(samples)}h", *[int(max(-1.0, min(1.0, v)) * 32767) for v in samples]))


def loop_to(noise: list[float], n: int, offset: int) -> list[float]:
    if not noise:
        die("noise file has no samples")
    return [noise[(offset + i) % len(noise)] for i in range(n)]


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--clean", required=True, help="clean speech WAV")
    p.add_argument("--noise", required=True, help="noise WAV (looped to fit)")
    p.add_argument("--snr-db", type=float, required=True,
                   help="target SNR in dB (negative = noise louder than speech)")
    p.add_argument("--out", required=True, help="output mixed WAV")
    p.add_argument("--seed", type=int, default=0, help="noise offset seed")
    args = p.parse_args()

    clean, clean_rate = read_mono_wav(args.clean)
    noise, noise_rate = read_mono_wav(args.noise)
    if noise_rate != clean_rate:
        die(f"rate mismatch: clean {clean_rate} Hz vs noise {noise_rate} Hz "
            "(convert both to 16 kHz first)")
    if rms(clean) == 0.0:
        die(f"{args.clean}: clean file is silent; SNR is undefined")

    rng = random.Random(args.seed)
    bed = loop_to(noise, len(clean), rng.randrange(len(noise)))
    target_ratio = 10.0 ** (args.snr_db / 20.0)
    gain = rms(clean) / (rms(bed) or 1e-9) / target_ratio
    mixed = [s + gain * n for s, n in zip(clean, bed)]
    write_mono_wav(args.out, mixed, clean_rate)

    actual = 20.0 * math.log10(rms(clean) / (rms([gain * n for n in bed]) or 1e-9))
    print(f"mixed {args.clean} + {args.noise} at {actual:.2f} dB -> {args.out}")


if __name__ == "__main__":
    main()
