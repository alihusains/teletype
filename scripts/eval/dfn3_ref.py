"""Reference DFN3 (dfn3_ll) run matching the Rust module exactly.

48 kHz, FFT 960 / hop 480, 32 ERB bands (linear edges 0-16 kHz, last
band to 24 kHz), 96 deep-filter bins, 5-tap complex deep filter.
Front end and back end replicate crates/teletype-core/src/audio/dfn3.rs.

Usage:
    python dfn3_ref.py <model_dir> <manifest.jsonl> <cache_dir> <out_wav_dir>

Reads each case's mix wav (16 kHz mono, path relative to
<cache_dir>/competing-0db/), denoises it, writes <out_wav_dir>/dfn3-<case>.wav
at 16 kHz for the eval driver to transcribe.
"""

import json
import os
import sys
import wave

import numpy as np

try:
    import onnxruntime as ort
except ImportError:
    print("onnxruntime is required: pip install onnxruntime", file=sys.stderr)
    raise

RATE = 48000
FFT = 960
HOP = 480
NB_ERB = 32
NB_DF = 96
DF_ORDER = 5
FREQ_BINS = FFT // 2 + 1


def read_wav16k(path):
    """Read a 16 kHz mono int16 wav and linearly upsample to 48 kHz."""
    with wave.open(path) as wf:
        if wf.getnchannels() != 1 or wf.getframerate() != 16000:
            raise ValueError(f"{path}: expected 16 kHz mono, got {wf.getframerate()} Hz {wf.getnchannels()} ch")
        raw = wf.readframes(wf.getnframes())
    x = np.frombuffer(raw, dtype=np.int16).astype(np.float32) / 32768.0
    n_out = len(x) * 3
    idx = np.arange(n_out) / 3.0
    i0 = idx.astype(int)
    i1 = np.minimum(i0 + 1, len(x) - 1)
    frac = idx - i0
    return (x[i0] * (1 - frac) + x[i1] * frac).astype(np.float32)


def erb_filter_bank(inverse):
    n = FREQ_BINS
    edges = [16000.0 * k / (NB_ERB - 1) for k in range(1, NB_ERB)]
    lo = [0.0] + edges
    hi = edges + [24000.0]
    freqs = np.arange(n) * 24000.0 / FFT
    fb = np.zeros((NB_ERB, n), dtype=np.complex64)
    for k in range(NB_ERB):
        lo_k, hi_k = lo[k], hi[k]
        v = np.where(
            (freqs > lo_k) & (freqs < hi_k),
            np.where(freqs <= (lo_k + hi_k) / 2, (freqs - lo_k) / ((hi_k - lo_k) / 2), (hi_k - freqs) / ((hi_k - lo_k) / 2)),
            0.0,
        ).astype(np.float32)
        fb[k] = v
    if inverse:
        fb = fb[::-1].copy()
    return fb


FB = erb_filter_bank(False)
WIN = np.hanning(FFT).astype(np.float32)


def process(x48, enc, erb_dec, df_dec):
    n_frames = (len(x48) - FFT) // HOP + 1
    idx = np.arange(FFT)[None, :] + HOP * np.arange(n_frames)[:, None]
    frames = x48[idx] * WIN
    spec = np.fft.rfft(frames, axis=1)

    erb = np.zeros((n_frames, NB_ERB), dtype=np.float32)
    for k in range(NB_ERB):
        mag = np.abs(spec * FB[k])
        erb[:, k] = np.sqrt(np.sum(mag ** 2, axis=1))

    erb_in = erb[None, None, :, :].astype(np.float32)
    spec_in = np.zeros((1, 2, n_frames, 96), dtype=np.float32)
    spec_in[0, 0] = spec.real[:, :96]
    spec_in[0, 1] = spec.imag[:, :96]
    e0, e1, e2, e3, emb, c0_orig, _lsnr = enc.run(None, {"feat_erb": erb_in, "feat_spec": spec_in})
    emb = emb[0].astype(np.float32)  # (T, 512)

    m = erb_dec.run(None, {
        "emb": emb[None, :, :].astype(np.float32),
        "e3": e3.astype(np.float32),
        "e2": e2.astype(np.float32),
        "e1": e1.astype(np.float32),
        "e0": e0.astype(np.float32),
    })[0]
    mask = m.reshape(-1, 32).astype(np.float32)
    if mask.shape[0] != n_frames:
        mask = mask[:n_frames]

    spec_out = spec.copy()
    for k in range(NB_ERB):
        spec_out[:, k] = spec_out[:, k] * mask[:, k]

    coefs = df_dec.run(None, {
        "emb": emb[None, :, :].astype(np.float32),
        "c0": c0_orig.astype(np.float32),
    })[0][0]  # (T, 96, 10)
    for t in range(n_frames):
        bins = spec_out[t, :NB_DF].copy()
        for j in range(NB_DF):
            acc = 0j
            for tap in range(DF_ORDER):
                c = complex(coefs[t, j, 2 * tap], coefs[t, j, 2 * tap + 1])
                src = bins[min(j + tap, NB_DF - 1)]
                acc += c * src
            spec_out[t, j] = acc

    out = np.zeros(len(x48), dtype=np.float32)
    norm = np.zeros(len(x48), dtype=np.float32)
    for t in range(n_frames):
        seg = np.fft.irfft(spec_out[t], FFT) * WIN
        s = t * HOP
        out[s:s + FFT] += seg
        norm[s:s + FFT] += WIN ** 2
    norm[norm < 1e-8] = 1.0
    return out / norm


def main():
    if len(sys.argv) != 5:
        print(__doc__, file=sys.stderr)
        return 1
    model_dir, manifest, cache_dir, out_dir = sys.argv[1:5]
    try:
        enc = ort.InferenceSession(f"{model_dir}/enc.onnx", providers=["CPUExecutionProvider"])
        erb_dec = ort.InferenceSession(f"{model_dir}/erb_dec.onnx", providers=["CPUExecutionProvider"])
        df_dec = ort.InferenceSession(f"{model_dir}/df_dec.onnx", providers=["CPUExecutionProvider"])
    except Exception as e:
        print(f"failed to load ONNX sessions from {model_dir}: {e}", file=sys.stderr)
        return 1

    try:
        os.makedirs(out_dir, exist_ok=True)
    except OSError as e:
        print(f"cannot create {out_dir}: {e}", file=sys.stderr)
        return 1
    n = 0
    try:
        with open(manifest) as mf:
            for line in mf:
                line = line.strip()
                if not line:
                    continue
                m = json.loads(line)
                cid = m["id"]
                mix = m["audio"]
                if not os.path.isabs(mix):
                    mix = f"{cache_dir}/competing-0db/{mix}"
                x48 = read_wav16k(mix)
                y48 = process(x48, enc, erb_dec, df_dec)
                y16 = y48[::3]
                s = np.clip(y16 * 32767, -32768, 32767).astype(np.int16)
                with wave.open(f"{out_dir}/dfn3-{cid}.wav", "wb") as wv:
                    wv.setnchannels(1)
                    wv.setsampwidth(2)
                    wv.setframerate(16000)
                    wv.writeframes(s.tobytes())
                n += 1
    except Exception as e:
        print(f"failed after {n} cases: {e}", file=sys.stderr)
        return 1
    print(f"wrote {n} denoised wavs to {out_dir}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
