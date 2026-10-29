#!/usr/bin/env python3
"""Benchmark an external ASR candidate on the Teletype eval corpora.

The Rust harness (`teletype-eval`) owns the scoring: WER, the verbatim
normalizer, the entity/vocab axes and the report format. This script only
produces **hypotheses** for a model that does not run inside the app, so the
candidate and the shipping engines are compared by identical code:

    python scripts/eval/bench_candidate_asr.py \
        --model aurora --set librispeech-clean \
        --out evals/runs/candidates/aurora/librispeech-clean/hypotheses.jsonl

    teletype-eval run --engine none \
        --set librispeech-clean \
        --hypotheses evals/runs/candidates/aurora/librispeech-clean/hypotheses.jsonl \
        --out evals/runs/candidates/aurora/librispeech-clean

Case ids follow the same rule the harness uses when several sets are merged
(`{set}-` prefix, added once), so a multi-set run lines up with
`teletype-eval run --set a,b --hypotheses ...`.

Latency is the per-clip model time only (audio decode excluded), the same
quantity the Rust measure mode records as `asr_ms`.

Supported candidates:

- `aurora`   — AuroraAI-Research/AuroraASR-1M (CTC + BPE, transformers,
               `trust_remote_code=True`).
- `nemotron` — nvidia/nemotron-3.5-asr-streaming-0.6b (RNNT, transformers
               `AutoModelForRNNT`).
- `zipformer`— the `pfluo/k2fsa-zipformer-chinese-english-mixed` checkpoint
               that `qualcomm/Zipformer` ships as QNN binaries, run from the
               sherpa-onnx ONNX export of the same weights.
- `lfm3`     — LiquidAI/LFM3-Audio-2B GGUF through a resident `llama-server`
               (llama.cpp b11960+). The adapter starts the server once, so
               `asr_ms` is per-request time with a warm model, matching the
               quantity the app would pay for a resident engine.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import wave
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

SAMPLE_RATE = 16_000


# ────────────────────────────────────────────────────────────────────────────
# corpus
# ────────────────────────────────────────────────────────────────────────────


@dataclass
class Case:
    id: str
    audio: Path
    reference: str


def cache_dir() -> Path:
    env = os.environ.get("TELETYPE_EVAL_CACHE", "").strip()
    if env:
        return Path(env)
    return Path.home() / ".cache" / "teletype-eval"


def load_set(name: str) -> list[Case]:
    """Read `<cache>/<name>/manifest.jsonl` into cases."""
    root = cache_dir() / name
    manifest = root / "manifest.jsonl"
    if not manifest.is_file():
        raise SystemExit(
            f"set {name!r} not found: {manifest} missing "
            "(run scripts/eval/fetch_datasets.sh to download it)"
        )
    cases: list[Case] = []
    with manifest.open(encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            row = json.loads(line)
            cases.append(
                Case(
                    id=row["id"],
                    audio=root / row["audio"],
                    reference=row.get("reference", ""),
                )
            )
    return cases


def read_wav(path: Path) -> np.ndarray:
    """16 kHz mono float32 samples.

    The eval corpora are 16 kHz mono LEI16; anything else is resampled
    linearly so a mismatch is a warning, never a crash.
    """
    with wave.open(str(path), "rb") as wav:
        channels = wav.getnchannels()
        width = wav.getsampwidth()
        rate = wav.getframerate()
        frames = wav.readframes(wav.getnframes())
    if width != 2:
        raise ValueError(f"{path} is {width * 8}-bit; only 16-bit is supported")
    samples = np.frombuffer(frames, dtype="<i2").astype(np.float32) / 32768.0
    if channels > 1:
        samples = samples.reshape(-1, channels).mean(axis=1)
    if rate != SAMPLE_RATE:
        print(
            f"warning: {path.name} is {rate} Hz, resampling to {SAMPLE_RATE} Hz",
            file=sys.stderr,
        )
        duration = samples.shape[0] / rate
        target = int(round(duration * SAMPLE_RATE))
        samples = np.interp(
            np.linspace(0, samples.shape[0] - 1, target, dtype=np.float64),
            np.arange(samples.shape[0], dtype=np.float64),
            samples.astype(np.float64),
        ).astype(np.float32)
    return samples


# ────────────────────────────────────────────────────────────────────────────
# model adapters
# ────────────────────────────────────────────────────────────────────────────


def make_aurora(model_dir: Path, threads: int, **_):
    """AuroraASR-1M: weight-shared Conformer-CTC with a 256-piece BPE."""
    import torch
    from transformers import AutoModelForCTC

    torch.set_num_threads(threads)
    model = AutoModelForCTC.from_pretrained(
        str(model_dir), trust_remote_code=True
    ).eval()
    spm_path = model_dir / "bpe256.model"
    if not spm_path.is_file():
        raise SystemExit(f"aurora: sentencepiece model not found: {spm_path}")

    def transcribe(samples: np.ndarray) -> str:
        with torch.inference_mode():
            return model.transcribe(samples, spm_path=str(spm_path))[0].strip()

    return transcribe


def make_nemotron(model_dir: Path, threads: int, **_):
    """Nemotron 3.5 ASR streaming 0.6B: cache-aware FastConformer + RNNT."""
    import torch
    from transformers import AutoModelForRNNT, AutoProcessor

    torch.set_num_threads(threads)
    processor = AutoProcessor.from_pretrained(str(model_dir))
    model = AutoModelForRNNT.from_pretrained(str(model_dir))
    model.eval()

    use_mps = hasattr(torch.backends, "mps") and torch.backends.mps.is_available()
    device = "mps" if use_mps else "cpu"
    model.to(device)
    print(f"nemotron: device={device} dtype={next(model.parameters()).dtype}")

    def transcribe(samples: np.ndarray) -> str:
        inputs = processor(
            samples,
            sampling_rate=SAMPLE_RATE,
            language="en-US",
            return_tensors="pt",
        )
        inputs = inputs.to(device)
        with torch.inference_mode():
            output = model.generate(**inputs, return_dict_in_generate=True)
        text = processor.decode(output.sequences, skip_special_tokens=True)
        return text.strip()

    return transcribe


def make_zipformer(model_dir: Path, threads: int, **_):
    """Qualcomm Zipformer weights, CPU-runnable via the sherpa-onnx export.

    `qualcomm/Zipformer` publishes only QNN/NPU binaries for Snapdragon
    chipsets; the model card names its checkpoint
    (`pfluo/k2fsa-zipformer-chinese-english-mixed`), and sherpa-onnx ships a
    CPU ONNX export of those same weights, which is what runs here.
    """
    import sherpa_onnx

    recognizer = sherpa_onnx.OnlineRecognizer.from_transducer(
        tokens=str(model_dir / "tokens.txt"),
        encoder=str(model_dir / "encoder-epoch-99-avg-1.onnx"),
        decoder=str(model_dir / "decoder-epoch-99-avg-1.onnx"),
        joiner=str(model_dir / "joiner-epoch-99-avg-1.onnx"),
        num_threads=threads,
        feature_dim=80,
        sample_rate=SAMPLE_RATE,
        decoding_method="greedy_search",
        model_type="zipformer",
        provider="cpu",
    )
    tail = np.zeros(int(0.66 * SAMPLE_RATE), dtype=np.float32)

    def transcribe(samples: np.ndarray) -> str:
        stream = recognizer.create_stream()
        stream.accept_waveform(SAMPLE_RATE, samples)
        stream.accept_waveform(SAMPLE_RATE, tail)
        stream.input_finished()
        while recognizer.is_ready(stream):
            recognizer.decode_stream(stream)
        return str(recognizer.get_result(stream)).strip()

    return transcribe


def make_phonon(model_dir: Path, threads: int, **_):
    """Phonon-2 Core ML (FermionResearch/Phonon-2-CoreML): 0.6B Parakeet-tdt-v3
    teacher distilled to 5-level ~2.1-bit QAT, 164 MB, English only.

    Uses the Python reference runner (phonon_coreml.py) which loads the Core ML
    encoder (Neural Engine) and the C decoder (libphonon_tdt.dylib) via ctypes.
    Requires Python 3.11+ with coremltools (the pip package must include the
    native libcoremlpython binding; Python 3.12+ from python.org does not ship
    it, but Homebrew Python 3.11 does).
    """
    import importlib.util
    spec = importlib.util.spec_from_file_location(
        "phonon_coreml", model_dir / "phonon_coreml.py"
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    m = mod.Phonon2CoreML(str(model_dir))

    def transcribe(samples: np.ndarray) -> str:
        result = m.transcribe(samples)
        return result["text"].strip()

    return transcribe


def make_lfm3(model_dir: Path, threads: int, language: str = "English"):
    """LiquidAI/LFM3-Audio-2B: a 2B audio LLM served by llama.cpp.

    This is not a CTC/RNNT engine: ASR is the system prompt
    `Perform ASR in <language>.` and the transcript is decoded text. The
    adapter keeps one `llama-server` resident so `asr_ms` excludes model
    load, like the shipped engines. llama.cpp b11960 (v0.8.0) is the first
    release that encodes LFM3 audio via the `mmproj` projector.
    """
    import atexit
    import base64
    import io
    import socket
    import subprocess
    import urllib.error
    import urllib.request

    weights = sorted(
        model_dir.glob("LFM3-Audio-2B-Q4_K_M.gguf")
    ) or sorted(p for p in model_dir.glob("*.gguf") if not p.name.startswith("mmproj"))
    projectors = sorted(model_dir.glob("mmproj-*.gguf"))
    if not weights or not projectors:
        raise SystemExit(
            f"lfm3: expected a non-mmproj .gguf and mmproj-*.gguf in {model_dir}"
        )

    server_bin = Path(
        os.environ.get(
            "LLAMA_SERVER",
            Path(__file__).resolve().parents[2]
            / ".tmp/asrbench/runtime/llama-b11960/llama-server",
        )
    )
    if not server_bin.is_file():
        raise SystemExit(
            f"lfm3: llama-server not found at {server_bin}; set LLAMA_SERVER "
            "(llama.cpp b11960+ is required for LFM3 audio)"
        )

    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    base_url = f"http://127.0.0.1:{port}"
    log_path = model_dir / "llama-server.log"
    log = log_path.open("wb")
    proc = subprocess.Popen(
        [
            str(server_bin),
            "-m", str(weights[0]),
            "--mmproj", str(projectors[0]),
            "-ngl", "99",
            "-c", "16384",
            "-t", str(threads),
            "--host", "127.0.0.1",
            "--port", str(port),
            "--no-webui",
        ],
        stdout=log,
        stderr=subprocess.STDOUT,
    )
    atexit.register(proc.terminate)
    atexit.register(log.close)

    deadline = time.time() + 180.0
    while True:
        if proc.poll() is not None:
            raise SystemExit(
                f"lfm3: llama-server exited with {proc.returncode}; see {log_path}"
            )
        try:
            with urllib.request.urlopen(f"{base_url}/health", timeout=1) as health:
                if health.status == 200:
                    break
        except (OSError, urllib.error.URLError):
            if time.time() > deadline:
                raise SystemExit(f"lfm3: llama-server not healthy; see {log_path}")
            time.sleep(0.25)

    system_prompt = f"Perform ASR in {language}."

    def transcribe(samples: np.ndarray) -> str:
        buffer = io.BytesIO()
        with wave.open(buffer, "wb") as wav:
            wav.setnchannels(1)
            wav.setsampwidth(2)
            wav.setframerate(SAMPLE_RATE)
            wav.writeframes(
                (np.clip(samples, -1.0, 1.0) * 32767.0).astype("<i2").tobytes()
            )
        seconds = samples.shape[0] / SAMPLE_RATE
        body = json.dumps(
            {
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "input_audio",
                                "input_audio": {
                                    "data": base64.b64encode(
                                        buffer.getvalue()
                                    ).decode("ascii"),
                                    "format": "wav",
                                },
                            }
                        ],
                    },
                ],
                "temperature": 0,
                "max_tokens": min(8192, max(256, int(seconds * 5))),
            }
        ).encode("utf-8")
        request = urllib.request.Request(
            f"{base_url}/v1/chat/completions",
            data=body,
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=900) as response:
            payload = json.load(response)
        return payload["choices"][0]["message"]["content"].strip()

    return transcribe


ADAPTERS = {
    "aurora": make_aurora,
    "nemotron": make_nemotron,
    "zipformer": make_zipformer,
    "phonon": make_phonon,
    "lfm3": make_lfm3,
}

DEFAULT_MODEL_DIRS = {
    "aurora": ".tmp/asrbench/models/auroraasr-1m",
    "nemotron": ".tmp/asrbench/models/nemotron-asr-streaming-0.6b",
    "zipformer": ".tmp/asrbench/models/zipformer-streaming-zh-en",
    "phonon": ".tmp/asrbench/models/phonon-2-coreml",
    "lfm3": ".tmp/asrbench/models/lfm3-audio-2b",
}

# The 18 speech languages LFM3-Audio-2B lists on its card, keyed by the
# FLEURS code the eval sets use.
LFM3_LANGUAGES = {
    "en": "English", "ar": "Arabic", "da": "Danish", "de": "German",
    "el": "Greek", "fi": "Finnish", "fr": "French", "he": "Hebrew",
    "it": "Italian", "ja": "Japanese", "ko": "Korean", "nl": "Dutch",
    "pl": "Polish", "pt": "Portuguese", "es": "Spanish", "sv": "Swedish",
    "tr": "Turkish", "zh": "Chinese",
}


def infer_language(set_names: list[str]) -> str:
    """Language for the ASR prompt, from a single `fleurs-<code>` set."""
    codes = {
        name.removeprefix("fleurs-")
        for name in set_names
        if name.startswith("fleurs-")
    }
    if len(codes) == 1:
        return LFM3_LANGUAGES.get(codes.pop(), "English")
    return "English"


# ────────────────────────────────────────────────────────────────────────────
# main
# ────────────────────────────────────────────────────────────────────────────


@dataclass
class Stats:
    cases: int = 0
    errors: list[str] = field(default_factory=list)
    audio_seconds: float = 0.0
    model_seconds: float = 0.0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--model",
        required=True,
        choices=sorted(ADAPTERS),
        help="candidate to benchmark",
    )
    parser.add_argument(
        "--model-dir",
        default=None,
        help="model directory (defaults to the repo-local download path)",
    )
    parser.add_argument(
        "--set",
        dest="sets",
        action="append",
        required=True,
        help="eval set name (repeatable; ids get the `{set}-` prefix when several sets are merged)",
    )
    parser.add_argument("--out", required=True, help="hypotheses JSONL path")
    parser.add_argument(
        "--limit", type=int, default=0, help="stop after N cases per set (0 = all)"
    )
    parser.add_argument(
        "--language",
        default=None,
        help="ASR prompt language (default: inferred from a fleurs-<code> set, else English)",
    )
    parser.add_argument("--threads", type=int, default=4, help="inference threads")
    args = parser.parse_args()

    model_dir = Path(
        args.model_dir
        or DEFAULT_MODEL_DIRS[args.model]
    )
    if not model_dir.is_dir():
        raise SystemExit(f"model directory not found: {model_dir}")

    multi = len(args.sets) > 1
    cases: list[Case] = []
    for name in args.sets:
        taken = 0
        for case in load_set(name):
            if args.limit and taken >= args.limit:
                break
            prefix = f"{name}-"
            if multi and not case.id.startswith(prefix):
                case.id = f"{prefix}{case.id}"
            cases.append(case)
            taken += 1

    print(f"model:   {args.model} ({model_dir})")
    print(f"sets:    {', '.join(args.sets)}")
    print(f"cases:   {len(cases)}")

    language = args.language or infer_language(args.sets)
    print(f"language: {language}")
    transcribe = ADAPTERS[args.model](model_dir, args.threads, language=language)

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)

    stats = Stats()
    started = time.time()
    with out_path.open("w", encoding="utf-8") as handle:
        for index, case in enumerate(cases, start=1):
            samples = read_wav(case.audio)
            audio_seconds = samples.shape[0] / SAMPLE_RATE
            hypothesis = ""
            asr_ms = None
            try:
                call_start = time.perf_counter()
                hypothesis = transcribe(samples)
                asr_ms = (time.perf_counter() - call_start) * 1000.0
            except Exception as exc:  # one bad clip must not abort the run
                stats.errors.append(f"{case.id}: {exc}")
                print(f"warning: {case.id} failed: {exc}", file=sys.stderr)

            stats.cases += 1
            stats.audio_seconds += audio_seconds
            if asr_ms is not None:
                stats.model_seconds += asr_ms / 1000.0

            row = {
                "id": case.id,
                "hypothesis": hypothesis,
                "asr_ms": asr_ms,
                "total_ms": asr_ms,
            }
            handle.write(json.dumps(row, ensure_ascii=False) + "\n")
            handle.flush()

            if index % 25 == 0 or index == len(cases):
                elapsed = time.time() - started
                rate = index / elapsed if elapsed else 0.0
                remaining = (len(cases) - index) / rate if rate else 0.0
                print(
                    f"[{index}/{len(cases)}] {elapsed:.0f}s elapsed, "
                    f"~{remaining:.0f}s left, "
                    f"rtf={stats.model_seconds / max(stats.audio_seconds, 1e-9):.3f}",
                    flush=True,
                )

    meta = {
        "model": args.model,
        "model_dir": str(model_dir),
        "sets": args.sets,
        "cases": stats.cases,
        "errors": stats.errors,
        "audio_seconds": round(stats.audio_seconds, 2),
        "model_seconds": round(stats.model_seconds, 2),
        "rtf": round(stats.model_seconds / max(stats.audio_seconds, 1e-9), 4),
        "wall_seconds": round(time.time() - started, 2),
        "threads": args.threads,
    }
    meta_path = out_path.with_suffix(".meta.json")
    meta_path.write_text(json.dumps(meta, indent=2), encoding="utf-8")
    print(json.dumps(meta, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
