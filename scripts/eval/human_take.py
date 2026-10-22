#!/usr/bin/env python3
"""Split one human-corpus take into per-case WAVs + build its manifest.

One take = a single continuous recording of the scripted book
(evals/human-corpus/scripted/book.md): 12 paragraphs separated by ~1s pauses.
This script segments the take on silence, names the segments case-01..case-12,
and emits a manifest.jsonl (teletype-eval classic layout) whose reference/clean
text is inlined from the shared ref/ directory.

Cases whose ref verbatim or clean is "PENDING" (the two spontaneous tasks,
until you transcribe the take) are skipped from the manifest; re-run this
script after filling the ref files to pick them up.

Usage:
  python3 scripts/eval/human_take.py take.wav take-1
  python3 scripts/eval/human_take.py take.wav take-1 --expected 12 --silence-db 38 --gap-ms 700
"""
from __future__ import annotations

import argparse
import json
import math
import os
import struct
import sys
import wave

REF_DEFAULT = os.path.join(
    os.path.dirname(__file__), "..", "..", "evals", "human-corpus", "scripted", "ref"
)
TAKES_DEFAULT = os.path.join(
    os.path.dirname(__file__), "..", "..", "evals", "human-corpus", "scripted", "takes"
)

def read_mono16(path: str) -> tuple[list[float], int]:
    try:
        return _read_pcm(path)
    except (OSError, wave.Error) as e:
        sys.exit(f"error: cannot open WAV {path}: {e}")

def _read_pcm(path: str) -> tuple[list[float], int]:
    with wave.open(path, "rb") as w:
        nch, sw, rate, nf = w.getnchannels(), w.getsampwidth(), w.getframerate(), w.getnframes()
        raw = w.readframes(nf)
    if sw not in (1, 2, 4):
        sys.exit(f"unsupported sample width {sw * 8} bits (use 16-bit WAV)")
    unit = sw * nch
    mono = []
    for i in range(0, len(raw) - unit + 1, unit):
        frame = raw[i : i + unit]
        vals = struct.unpack(f"<{nch}{'h' if sw == 2 else ('i' if sw == 4 else 'b')}", frame)
        peak = max(vals, key=lambda v: abs(v))
        scale = (1 << (sw * 8 - 1)) if sw > 1 else 128
        mono.append(peak / scale)
    return mono, rate

def resampleLinear(x: list[float], src_rate: int, dst_rate: int) -> list[float]:
    if src_rate == dst_rate:
        return x
    n_out = max(1, int(len(x) * dst_rate / src_rate))
    step = len(x) / n_out
    out = []
    for i in range(n_out):
        pos = i * step
        j = min(int(pos), len(x) - 1)
        frac = pos - j
        a, b = x[j], x[j + 1] if j + 1 < len(x) else x[j]
        out.append(a + (b - a) * frac)
    return out

def segments(samples: list[float], rate: int, silence_db: float, gap_ms: int, min_seg_ms: int) -> list[tuple[int, int]]:
    win = max(1, int(rate * 0.03))
    hop = max(1, int(rate * 0.01))
    thr = 10 ** (-silence_db / 20.0)
    speech = []
    for start in range(0, len(samples) - win, hop):
        s = samples[start : start + win]
        rms = math.sqrt(sum(v * v for v in s) / len(s))
        speech.append(1 if rms >= thr else 0)
    # merge: a gap of >= gap_ms sustained silence ends a segment
    gap_windows = max(1, int(gap_ms / 10.0))
    runs: list[list[int]] = []
    run: list[int] = []
    silent = 0
    for i, is_s in enumerate(speech):
        if is_s:
            if run:
                run.append(i)
            else:
                run = [i]
            silent = 0
        else:
            silent += 1
            if run and silent >= gap_windows:
                runs.append(run)
                run = []
                silent = 0
    if run:
        runs.append(run)
    min_hops = max(1, int(min_seg_ms / 10.0))
    segs = []
    for r in runs:
        if len(r) < min_hops:
            continue
        s0, s1 = r[0], r[-1] + 1
        segs.append((max(0, s0 * hop - int(rate * 0.15)), min(len(samples), s1 * hop + int(rate * 0.15))))
    return segs

def write_wav(path: str, samples: list[float], rate: int) -> None:
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        frames = b"".join(struct.pack("<h", max(-32768, min(32767, int(v * 32767)))) for v in samples)
        w.writeframes(frames)

def read_ref(path: str) -> str:
    try:
        with open(path) as f:
            return f.read().strip()
    except OSError as e:
        sys.exit(f"error: cannot read {path}: {e}")

def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("take_wav")
    ap.add_argument("take_name")
    ap.add_argument("--expected", type=int, default=12)
    ap.add_argument("--silence-db", type=float, default=38.0)
    ap.add_argument("--gap-ms", type=int, default=700)
    ap.add_argument("--min-seg-ms", type=int, default=400)
    ap.add_argument("--ref-dir", default=REF_DEFAULT)
    ap.add_argument("--takes-dir", default=TAKES_DEFAULT)
    a = ap.parse_args()

    out_dir = os.path.abspath(os.path.join(a.takes_dir, a.take_name))
    try:
        os.makedirs(out_dir, exist_ok=True)
    except OSError as e:
        sys.exit(f"error: cannot create {out_dir}: {e}")
    raw, rate = read_mono16(a.take_wav)
    x16 = resampleLinear(raw, rate, 16000)
    rate = 16000
    segs = segments(x16, rate, a.silence_db, a.gap_ms, a.min_seg_ms)
    print(f"segments found: {len(segs)} (expect {a.expected})", file=sys.stderr)
    if len(segs) != a.expected:
        print("WARNING: segment count mismatch — re-check take pacing (≈1 s pauses) or tune --gap-ms/--silence-db", file=sys.stderr)

    ref = os.path.abspath(a.ref_dir)
    n_cases = 0
    try:
        with open(os.path.join(out_dir, "manifest.jsonl"), "w") as mf:
            for i, (s0, s1) in enumerate(segs, start=1):
                cid = f"case-{i:02d}"
                p = os.path.join(ref, f"{cid}.clean.txt")
                if not os.path.exists(p):
                    break
                clean = read_ref(p)
                verbatim = read_ref(os.path.join(ref, f"{cid}.verbatim.txt"))
                try:
                    meta = json.loads(read_ref(os.path.join(ref, f"{cid}.meta.json")))
                except json.JSONDecodeError as e:
                    sys.exit(f"error: bad meta for {cid}: {e}")
                wpath = os.path.join(out_dir, f"{cid}.wav")
                write_wav(wpath, x16[s0:s1], rate)
                dur = (s1 - s0) / rate
                print(f"  {cid}  {dur:5.2f}s  ctx={meta['app_context']:<6} vocab={meta.get('vocab', [])}", file=sys.stderr)
                if verbatim == "PENDING" or clean == "PENDING":
                    print(f"  {cid}: ref PENDING — skipped in manifest (fill refs, re-run)", file=sys.stderr)
                    continue
                mf.write(json.dumps({
                    "id": cid,
                    "audio": f"{cid}.wav",
                    "reference": verbatim.lower(),
                    "clean": clean.lower(),
                    "tags": [f"human:{meta['app_context']}"],
                    "human": True,
                }) + "\n")
                n_cases += 1
    except OSError as e:
        sys.exit(f"error: cannot write manifest: {e}")
    print(f"wrote {n_cases} cases -> {out_dir}", file=sys.stderr)
    if len(segs) != a.expected:
        sys.exit(2)

if __name__ == "__main__":
    main()
