#!/usr/bin/env python3
"""Build the earnings22-vocab eval set (plan P0.4).

Problem: ArtificialAnalysis/Earnings22-Cleaned-AA-chunked ships chunk audio
but NO per-chunk transcripts (its card: references come from the cleaned
parent transcripts, aligned in AA's own pipeline). So:
  1. load the 6 parent transcripts from Earnings22-Cleaned-AA (has `id`,
     `duration`, `transcript`),
  2. map each VAD chunk to its parent and apportion transcript words
     proportionally to chunk durations, snapping to word boundaries,
  3. download chunk wavs, convert to 16 kHz mono, sample 100 segments
     (fixed seed), write manifest.jsonl + vocab.json.

Word-boundary snapping keeps references clean; only boundary-word
attribution is approximate (documented, fine for vocab-recall).
Idempotent: skips when manifest.jsonl + vocab.json + .done all exist.
"""
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from subset import (  # noqa: E402
    check_ffmpeg,
    clean_id,
    convert_to_wav,
    deterministic_sample,
    extract_vocab_terms,
    write_manifest,
)

CHUNKED_REPO = "ArtificialAnalysis/Earnings22-Cleaned-AA-chunked"
CHUNKED_JSONL = "earnings22_cleaned_aa_chunked_v1.jsonl"
PARENT_DATASET = "ArtificialAnalysis/Earnings22-Cleaned-AA"
N_SEGMENTS = 100
SEED = int(os.environ.get("EVAL_SEED", "20261014"))


def main() -> None:
    out_dir = sys.argv[1] if len(sys.argv) > 1 else os.path.join(
        os.environ.get("TELETYPE_EVAL_CACHE", os.path.expanduser("~/.cache/teletype-eval")),
        "earnings22-vocab",
    )
    manifest = os.path.join(out_dir, "manifest.jsonl")
    vocab_path = os.path.join(out_dir, "vocab.json")
    done = os.path.join(out_dir, ".done")
    if all(os.path.exists(p) for p in (manifest, vocab_path, done)):
        print(f"have earnings22-vocab ({out_dir})")
        return
    check_ffmpeg()
    from datasets import load_dataset
    from huggingface_hub import hf_hub_download

    print("loading chunk index ...")
    chunk_path = hf_hub_download(CHUNKED_REPO, CHUNKED_JSONL, repo_type="dataset")
    chunks = [json.loads(l) for l in open(chunk_path, encoding="utf-8") if l.strip()]
    print(f"loading parent transcripts from {PARENT_DATASET} ...")
    parents = {}
    for row in load_dataset(PARENT_DATASET, split="test", streaming=True):
        parents[row["id"]] = (row["transcript"], float(row["duration"]))

    by_parent: dict[str, list[dict]] = {}
    for c in chunks:
        by_parent.setdefault(c["parent_id"], []).append(c)

    segments = []
    missing_parents = 0
    for pid, items in sorted(by_parent.items()):
        parent = parents.get(pid)
        if parent is None:
            missing_parents += 1
            continue
        transcript, total_dur = parent
        words = transcript.split()
        items.sort(key=lambda c: c["chunk_index"])
        total_dur = total_dur or sum(c["duration"] for c in items)
        pos = 0
        for ci, c in enumerate(items):
            frac = (c["duration"] / total_dur) if total_dur > 0 else 1.0 / len(items)
            if ci == len(items) - 1:
                end = len(words)  # last chunk takes the tail (no word lost)
            else:
                end = min(len(words), pos + max(1, round(frac * len(words))))
            ref = " ".join(words[pos:end])
            pos = end
            if not ref.strip():
                continue
            segments.append({"reference": ref, "tags": ["earnings22-vocab", pid],
                             "file_name": c["file_name"]})
    if missing_parents:
        print(f"warning: {missing_parents} chunk parents missing transcripts")
    if not segments:
        raise SystemExit("no earnings22 segments built")
    picked = deterministic_sample(segments, N_SEGMENTS, SEED)
    # Download + convert only the sampled chunks.
    audio_dir = os.path.join(out_dir, "audio")
    os.makedirs(audio_dir, exist_ok=True)
    for s in picked:
        audio_path = hf_hub_download(CHUNKED_REPO, f"audio/{s.pop('file_name')}",
                                     repo_type="dataset")
        dest = os.path.join(audio_dir, clean_id(
            f"earn22-tmp-{len(os.listdir(audio_dir))}") + ".wav")
        convert_to_wav(audio_path, dest)
        s["audio_tmp"] = dest
    # per-file vocab lists from the chunk references — computed on the
    # ORIGINAL case (write_manifest lowercases, which would erase all
    # capitalized-phrase terms), then keyed to manifest ids in order.
    vocab_terms = [extract_vocab_terms(s["reference"]) for s in picked]
    rows = [{"reference": s["reference"], "tags": s["tags"]} for s in picked]
    write_manifest(rows, out_dir, "earnings22-vocab")
    # relocate converted audio to the manifest's audio/ names
    audio_dir = os.path.join(out_dir, "audio")
    for s, rid in zip(picked, range(1, len(picked) + 1)):
        want = os.path.join(audio_dir, clean_id(f"earnings22-vocab-{rid:04d}") + ".wav")
        src = s["audio_tmp"]
        if os.path.abspath(src) != os.path.abspath(want):
            os.replace(src, want)
    # sweep strays (tmp names + leftovers from interrupted runs)
    keep = {os.path.join(audio_dir, clean_id(f"earnings22-vocab-{i + 1:04d}") + ".wav")
            for i in range(len(picked))}
    for f in os.listdir(audio_dir):
        p = os.path.join(audio_dir, f)
        if p not in keep:
            os.remove(p)
    # per-file vocab lists from the chunk references
    vocab = {}
    ids = []
    for line in open(manifest, encoding="utf-8"):
        row = json.loads(line)
        ids.append(row["id"])
    assert len(ids) == len(vocab_terms), (len(ids), len(vocab_terms))
    for rid, terms in zip(ids, vocab_terms):
        vocab[rid] = terms
    json.dump(vocab, open(vocab_path, "w", encoding="utf-8"), indent=2)
    print(f"segments: {len(picked)}, vocab terms: {sum(len(v) for v in vocab.values())}")
    open(done, "w").write("built by build_earnings22_segments.py\n")


if __name__ == "__main__":
    main()
