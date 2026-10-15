#!/usr/bin/env python3
"""Build the earnings22-vocab eval set (plan P0.4) — one case per call.

The `ArtificialAnalysis/Earnings22-Cleaned-AA-chunked` repo ships chunk audio
plus start/end timings, but **no per-chunk transcripts**; its card says
references are "aligned during the Artificial Analysis evaluation pipeline".
An earlier builder apportioned the parent transcript's words to chunks
proportionally to chunk duration. That assumption breaks wherever the local
speaking rate deviates from the call average, and the failure is systematic:
misaligned chunks show the model transcribing the middle/tail of the assigned
window (measured 2026-10-15: per-chunk WER 0.82 on parent 4471586, with
hypotheses matching the *end* of each reference window). Root cause: the
chunk boundaries are Silero-VAD speech segments (README), so word position
inside a call is not a linear function of elapsed audio.

Scoring changes to the unit the data actually supports: **the whole call**.
The 341 chunk wavs tile each parent losslessly (chunk k ends exactly where
chunk k+1 starts; verified on the jsonl), so concatenating a call's chunks in
chunk_index order reconstructs the cleaned parent audio, and the cleaned
parent transcript is then an exact reference — no alignment of any kind is
needed. This also matches how AA itself scores Earnings22 (per-call WER on
their leaderboard). Verified 2026-10-15: parent 4471586 scores 0.1034
verbatim_micro whole-call (vs 0.82 with the chunk apportionment) in ~24 s of
Parakeet inference — see the learning note in `.agents/brain/learnings/`.

Output contract (same as the other sets): manifest.jsonl, vocab.json,
audio/earnings22-vocab-000N.wav, .done. Idempotent via the `.done` marker;
the marker records the builder version so a stale layout from the old
chunk-based builder is rebuilt instead of skipped.
"""
import json
import os
import shutil
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from subset import (  # noqa: E402
    check_ffmpeg,
    clean_id,
    convert_to_wav,
    extract_vocab_terms,
    write_manifest,
)

CHUNKED_REPO = "ArtificialAnalysis/Earnings22-Cleaned-AA-chunked"
CHUNKED_JSONL = "earnings22_cleaned_aa_chunked_v1.jsonl"
PARENT_REPO = "ArtificialAnalysis/Earnings22-Cleaned-AA"
DONE_MARKER = "build_earnings22_calls.py v1\n"


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: build_earnings22_calls.py <out_dir>")
    check_ffmpeg()
    try:
        from datasets import load_dataset  # noqa: PLC0415
        from huggingface_hub import hf_hub_download  # noqa: PLC0415
    except ImportError as e:  # pragma: no cover - environment guard
        raise SystemExit(f"missing dependency (pip install datasets huggingface_hub): {e}")

    out_dir = sys.argv[1]
    done = os.path.join(out_dir, ".done")
    manifest = os.path.join(out_dir, "manifest.jsonl")
    vocab_path = os.path.join(out_dir, "vocab.json")
    if (
        os.path.exists(done)
        and open(done, encoding="utf-8").read() == DONE_MARKER
        and os.path.exists(manifest)
        and os.path.exists(vocab_path)
    ):
        print(f"have earnings22-vocab ({out_dir})")
        return

    os.makedirs(os.path.join(out_dir, "audio"), exist_ok=True)

    # 1. parent transcripts (the exact references)
    parents: list[dict] = []
    for row in load_dataset(PARENT_REPO, split="test", streaming=True):
        parents.append({"id": str(row["id"]), "duration": float(row["duration"]),
                        "transcript": str(row["transcript"])})
    parents.sort(key=lambda p: p["id"])
    if not parents:
        raise SystemExit("no earnings22 parents found")

    # 2. chunk index (audio file names + order)
    chunk_path = hf_hub_download(CHUNKED_REPO, CHUNKED_JSONL, repo_type="dataset")
    by_parent: dict[str, list[dict]] = {}
    for line in open(chunk_path, encoding="utf-8"):
        if not line.strip():
            continue
        c = json.loads(line)
        by_parent.setdefault(c["parent_id"], []).append(c)

    tmp = os.path.join(out_dir, ".chunks-tmp")
    os.makedirs(tmp, exist_ok=True)

    rows: list[dict] = []
    vocab_terms: list[list[str]] = []
    for i, parent in enumerate(parents):
        pid = parent["id"]
        items = sorted(by_parent.get(pid, []), key=lambda c: c["chunk_index"])
        if not items:
            raise SystemExit(f"no chunks for parent {pid}")
        list_path = os.path.join(tmp, f"{clean_id(pid)}.list")
        with open(list_path, "w", encoding="utf-8") as lf:
            for j, c in enumerate(items):
                part = os.path.join(tmp, f"{clean_id(pid)}-{j:04d}.wav")
                if not os.path.exists(part):
                    src = hf_hub_download(CHUNKED_REPO, f"audio/{c['file_name']}",
                                          repo_type="dataset")
                    convert_to_wav(src, part)
                lf.write(f"file '{part}'\n")
        dest = os.path.join(out_dir, "audio",
                            clean_id(f"earnings22-vocab-{i + 1:04d}") + ".wav")
        subprocess.run(
            ["ffmpeg", "-v", "error", "-y", "-f", "concat", "-safe", "0",
             "-i", list_path, "-c", "copy", dest],
            check=True,
        )
        # Sanity: the chunks should tile the parent (~1 s of VAD rounding).
        got = float(subprocess.run(
            ["ffprobe", "-v", "error", "-show_entries", "format=duration",
             "-of", "csv=p=0", dest],
            capture_output=True, text=True, check=True).stdout.strip())
        if abs(got - parent["duration"]) > 5.0:
            print(f"warning: {pid}: assembled {got:.1f}s vs parent "
                  f"{parent['duration']:.1f}s", file=sys.stderr)
        rows.append({"reference": parent["transcript"], "tags": ["earnings22-vocab", pid]})
        vocab_terms.append(extract_vocab_terms(parent["transcript"]))

    # 3. manifest + vocab.json keyed to the manifest ids (write_manifest
    # lowercases references; vocab terms are extracted from the originals).
    write_manifest(rows, out_dir, "earnings22-vocab")
    ids = [json.loads(line)["id"] for line in open(manifest, encoding="utf-8")]
    assert len(ids) == len(vocab_terms), (len(ids), len(vocab_terms))
    vocab = {rid: terms for rid, terms in zip(ids, vocab_terms)}
    json.dump(vocab, open(vocab_path, "w", encoding="utf-8"), indent=2)

    # 4. sweep strays (tmp names + leftovers from interrupted/older builds)
    audio_dir = os.path.join(out_dir, "audio")
    keep = {os.path.join(audio_dir, clean_id(f"earnings22-vocab-{i + 1:04d}") + ".wav")
            for i in range(len(rows))}
    for f in os.listdir(audio_dir):
        p = os.path.join(audio_dir, f)
        if p not in keep:
            os.remove(p)
    shutil.rmtree(tmp, ignore_errors=True)

    with open(done, "w", encoding="utf-8") as f:
        f.write(DONE_MARKER)
    print(f"calls: {len(rows)}, vocab terms: {sum(len(v) for v in vocab.values())}")


if __name__ == "__main__":
    main()
