#!/usr/bin/env python3
"""Deterministic subset sampler + manifest/vocab writer for teletype-eval.

One tool, three jobs (kept together so the sampling seed and the manifest
schema live in one place):

  subset.py sample --from-jsonl <lines> --out <set-dir> --set <name>
                   --n <k> --seed <int> [--vocab] [--convert]

    Read `{audio, reference, tags?}` JSON lines, take a deterministic
    `random.Random(seed)` sample of `--n` rows, convert each clip to 16 kHz
    mono WAV with ffmpeg, and write `<out>/manifest.jsonl` (+ audio/) plus,
    with `--vocab`, `<out>/vocab.json` (see `extract_vocab_terms`).

  subset.py hf --dataset <id> --config <name> --split <s>
               --audio-col <c> --text-col <c> --out ... (same as sample)

    Same, but streams rows from a Hugging Face dataset (`datasets` package).
    Rows whose audio cannot be decoded are skipped with a warning and the
    sample is topped up from the stream (still deterministic: the draw order
    is seeded, skips don't consume draws).

  subset.py noise --clean-set <dir> --musan <dir> --out <dir>
                  --n 100 --snrs 10,5,0 --seed <int>

    MUSAN mixes: pick `--n` clips from a LibriSpeech test-other set, mix one
    random MUSAN file per clip at each SNR in `--snrs`, and write a set dir
    per SNR (`<out>-10db/`, …) with manifests that reuse the clean
    references.

`extract_vocab_terms` is a heuristic, not a label: capitalized phrases,
ALL-CAPS tokens (tickers, acronyms), and money amounts. It exists so
Earnings-22-style sets get a *per-file* vocabulary list to boost on; the
eval reports recall against it, and a human should eyeball `vocab.json`
before quoting numbers.
"""

from __future__ import annotations

import argparse
import json
import os
import random
import re
import shutil
import subprocess
import sys
import tempfile

MANIFEST_NAME = "manifest.jsonl"


def die(msg: str) -> "NoReturn":
    print(f"subset.py: error: {msg}", file=sys.stderr)
    sys.exit(1)


def check_ffmpeg() -> None:
    if shutil.which("ffmpeg") is None:
        die("ffmpeg is required (audio conversion); install it and re-run")


def convert_to_wav(src: str, dest_16k_wav: str) -> None:
    """Convert any ffmpeg-readable audio to 16 kHz mono 16-bit WAV."""
    os.makedirs(os.path.dirname(dest_16k_wav), exist_ok=True)
    cmd = [
        "ffmpeg", "-y", "-v", "error", "-i", src,
        "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", dest_16k_wav,
    ]
    try:
        subprocess.run(cmd, check=True)
    except subprocess.CalledProcessError as e:
        die(f"ffmpeg failed for {src}: {e}")


def clean_id(raw: str) -> str:
    cleaned = re.sub(r"[^a-z0-9-]", "-", raw.lower()).strip("-")
    cleaned = re.sub(r"-+", "-", cleaned)
    return cleaned or "clip"


def write_manifest(rows: list[dict], out_dir: str, set_name: str) -> None:
    os.makedirs(os.path.join(out_dir, "audio"), exist_ok=True)
    path = os.path.join(out_dir, MANIFEST_NAME)
    with open(path, "w", encoding="utf-8") as f:
        for i, row in enumerate(rows):
            rid = f"{set_name}-{i + 1:04d}"
            entry = {
                "id": clean_id(rid),
                "audio": f"audio/{clean_id(rid)}.wav",
                "reference": row["reference"].strip().lower(),
                "tags": row.get("tags", [set_name]),
                "human": False,
            }
            if "clean" in row and row["clean"]:
                entry["clean"] = row["clean"].strip().lower()
            f.write(json.dumps(entry) + "\n")
    print(f"wrote {len(rows)} rows to {path}")


def deterministic_sample(rows: list[dict], n: int, seed: int) -> list[dict]:
    rng = random.Random(seed)
    if len(rows) <= n:
        print(f"note: only {len(rows)} rows available (< {n} requested); using all")
        order = rows[:]
        rng.shuffle(order)
        return order
    idx = rng.sample(range(len(rows)), n)
    return [rows[i] for i in sorted(idx)]


def extract_vocab_terms(reference: str) -> list[str]:
    """Heuristic per-file vocabulary: capitalized phrases, tickers/acronyms,
    money amounts. See module docstring for the caveats."""
    terms: list[str] = []
    seen: set[str] = set()

    def add(term: str) -> None:
        term = term.strip(" ,.;:!?()[]\"'")
        if len(term) >= 2 and term.lower() not in seen:
            seen.add(term.lower())
            terms.append(term)

    # Capitalized phrases ("Priya Sharma"). A phrase at the very start of the
    # reference loses its first word: that word is capitalized for being
    # sentence-initial ("Ask Priya Sharma" → "Priya Sharma").
    for m in re.finditer(r"\b([A-Z][a-z]+(?:\s+[A-Z][a-z]+)+)\b", reference):
        phrase = m.group(1)
        if m.start() == 0:
            phrase = " ".join(phrase.split()[1:])
        if phrase:
            add(phrase)
    # ALL-CAPS tokens of length >= 2 (tickers, acronyms, orgs).
    for m in re.finditer(r"\b([A-Z]{2,}(?:\.[A-Z]+)?)\b", reference):
        if m.group(1) not in ("I",):
            add(m.group(1))
    # Caps-led alphanumeric tokens (model names, regions: S3, H100, us-east-1
    # keeps its letters; the digits ride along in the match).
    for m in re.finditer(r"\b([A-Z][A-Z0-9]{1,}(?:[-_][A-Z0-9]+)*)\b", reference):
        add(m.group(1))
    # Single capitalized words in tech/finance position are covered by the
    # phrase pass only when paired; also catch lone capitalized words that
    # are not sentence-initial.
    for m in re.finditer(r"(?<!^)(?<=\s)([A-Z][a-z]{2,})\b", reference):
        add(m.group(1))
    # Money amounts ("$80 million", "42%").
    for m in re.finditer(r"\$[\d,]+(?:\.\d+)?(?:\s+million|\s+billion)?\b", reference):
        add(m.group(0))
    for m in re.finditer(r"\b\d+(?:\.\d+)?%", reference):
        add(m.group(0))
    return terms


def cmd_sample(args: argparse.Namespace) -> None:
    check_ffmpeg()
    rows: list[dict] = []
    with open(args.from_jsonl, encoding="utf-8") as f:
        for lineno, line in enumerate(f, 1):
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except json.JSONDecodeError as e:
                die(f"{args.from_jsonl}:{lineno}: bad JSON: {e}")
            if "audio" not in obj or "reference" not in obj:
                die(f"{args.from_jsonl}:{lineno}: need 'audio' + 'reference' keys")
            rows.append(obj)
    picked = deterministic_sample(rows, args.n, args.seed)
    staged: list[dict] = []
    for row in picked:
        rid = f"{args.set}-{len(staged) + 1:04d}"
        dest = os.path.join(args.out, "audio", clean_id(rid) + ".wav")
        convert_to_wav(row["audio"], dest)
        staged.append(row)
    write_manifest(staged, args.out, args.set)
    if args.vocab:
        vocab = {}
        for i, row in enumerate(staged):
            rid = clean_id(f"{args.set}-{i + 1:04d}")
            vocab[rid] = extract_vocab_terms(row["reference"])
        with open(os.path.join(args.out, "vocab.json"), "w", encoding="utf-8") as f:
            json.dump(vocab, f, indent=2)
        print(f"wrote vocab.json ({sum(len(v) for v in vocab.values())} terms)")


def resolve_config(dataset: str, want: str | None) -> str | None:
    """Fuzzy-match a requested config (e.g. `en`) against the dataset's real
    config names (e.g. FLEURS' `en_us`). Exact match wins; then common
    locale variants; then a unique prefix match. Returns None (→ default
    config) when `want` is None."""
    if not want:
        return None
    try:
        from datasets import get_dataset_config_names
    except ImportError:
        return want  # caller already errors on missing `datasets`
    try:
        available = get_dataset_config_names(dataset, trust_remote_code=False)
    except Exception as e:  # noqa: BLE001 - offline or unknown dataset
        print(f"warning: cannot list configs for {dataset} ({e}); trying {want!r} as-is",
              file=sys.stderr)
        return want
    if want in available:
        return want
    for cand in (f"{want}_us", f"{want}_gb", f"{want}_419", f"{want}_{want}"):
        if cand in available:
            print(f"note: config {want!r} → {cand!r}", file=sys.stderr)
            return cand
    prefixed = [c for c in available if c == want or c.startswith(want + "_")]
    if len(prefixed) == 1:
        print(f"note: config {want!r} → {prefixed[0]!r}", file=sys.stderr)
        return prefixed[0]
    die(f"dataset {dataset} has no config like {want!r} "
        f"(available include: {', '.join(available[:12])})")
    return want


def cmd_hf(args: argparse.Namespace) -> None:
    try:
        from datasets import load_dataset
    except ImportError:
        die("the 'datasets' package is required (pip install datasets) for --dataset")
    check_ffmpeg()
    config = resolve_config(args.dataset, args.config)
    ds = load_dataset(
        args.dataset, config, split=args.split, streaming=True, trust_remote_code=False
    )
    # `datasets` >= 4 decodes audio through torchcodec (a multi-GB torch
    # dependency); this harness already needs ffmpeg, so keep the raw file
    # bytes/path and let `convert_to_wav` decode. Silently a no-op on old
    # `datasets` where the column was never auto-decoded.
    try:
        from datasets import Audio

        ds = ds.cast_column(args.audio_col, Audio(decode=False))
    except Exception as e:  # noqa: BLE001 - older datasets: raw column already
        print(f"note: could not disable audio decoding ({e}); continuing", file=sys.stderr)
    tmp = tempfile.mkdtemp(prefix="teletype-eval-hf-")
    rng = random.Random(args.seed)
    # Reservoir sampling over (at most --max-rows of) the stream: unbiased
    # within the scanned prefix, deterministic for a fixed seed. The cap
    # exists so a 100k-hour stream doesn't fill tmp; the default covers the
    # full test splits of every set in fetch_datasets.sh.
    reservoir: list[dict] = []
    seen = 0
    skipped = 0
    first_error: str | None = None
    for ex in ds:
        if seen >= args.max_rows:
            break
        try:
            text = ex[args.text_col]
            audio = ex[args.audio_col]
            if not text or not audio:
                skipped += 1
                continue
            path = audio.get("path") if isinstance(audio, dict) else None
            raw = audio.get("bytes") if isinstance(audio, dict) else None
            array = audio.get("array") if isinstance(audio, dict) else None
            sr = audio.get("sampling_rate", 16000) if isinstance(audio, dict) else 16000
            if path and os.path.exists(path):
                src = path
            elif raw:
                # Raw container bytes (wav/flac/mp3/…): keep the original
                # extension when the source path had one; ffmpeg sniffs the
                # content either way.
                ext = os.path.splitext(path or "")[1] or ".bin"
                src = os.path.join(tmp, f"row-{seen}{ext}")
                with open(src, "wb") as f:
                    f.write(raw)
            elif array is not None:
                import struct
                import wave

                src = os.path.join(tmp, f"row-{seen}.wav")
                peak = max([abs(x) for x in array], default=0.0) or 1.0
                with wave.open(src, "wb") as w:
                    w.setnchannels(1)
                    w.setsampwidth(2)
                    w.setframerate(int(sr))
                    w.writeframes(
                        struct.pack(
                            f"<{len(array)}h",
                            *[int(x / peak * 32767) for x in array],
                        )
                    )
            else:
                skipped += 1
                continue
            cand = {"src": src, "reference": str(text), "tags": [args.set]}
            if len(reservoir) < args.n:
                reservoir.append(cand)
            else:
                j = rng.randrange(seen + 1)
                if j < args.n:
                    reservoir[j] = cand
            seen += 1
        except Exception as e:  # noqa: BLE001 - one bad row must not kill a GB download
            skipped += 1
            if first_error is None:
                first_error = str(e)
            print(f"warning: skipping row ({e})", file=sys.stderr)
    if not reservoir:
        hint = f" first error: {first_error}" if first_error else ""
        die(f"no usable rows from {args.dataset}/{args.config} (skipped {skipped}).{hint} "
            f"Check --split/--audio-col/--text-col against the dataset viewer.")
    print(f"scanned {seen} rows, kept {len(reservoir)}, skipped {skipped}")
    staged: list[dict] = []
    for cand in reservoir:
        rid = f"{args.set}-{len(staged) + 1:04d}"
        dest = os.path.join(args.out, "audio", clean_id(rid) + ".wav")
        convert_to_wav(cand["src"], dest)
        staged.append({"reference": cand["reference"], "tags": cand["tags"]})
    write_manifest(staged, args.out, args.set)


def cmd_noise(args: argparse.Namespace) -> None:
    # Deferred import: numpy is optional; without it use mix_noise.py in a
    # loop (the fetch script does that). This entry point exists so a numpy
    # environment gets the fast path.
    die("noise mixing runs through scripts/eval/mix_noise.py (one clip per call); "
        "see fetch_datasets.sh musan_set()")


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=__doc__)
    sub = p.add_subparsers(dest="cmd", required=True)

    s = sub.add_parser("sample", help="sample a JSONL row file into a set dir")
    s.add_argument("--from-jsonl", required=True)
    s.add_argument("--out", required=True)
    s.add_argument("--set", required=True)
    s.add_argument("--n", type=int, required=True)
    s.add_argument("--seed", type=int, default=20261014)
    s.add_argument("--vocab", action="store_true")
    s.set_defaults(fn=cmd_sample)

    h = sub.add_parser("hf", help="sample a Hugging Face dataset into a set dir")
    h.add_argument("--dataset", required=True)
    h.add_argument("--config", default=None)
    h.add_argument("--split", default="test")
    h.add_argument("--audio-col", default="audio")
    h.add_argument("--text-col", default="text")
    h.add_argument("--out", required=True)
    h.add_argument("--set", required=True)
    h.add_argument("--n", type=int, required=True)
    h.add_argument("--seed", type=int, default=20261014)
    h.add_argument("--max-rows", type=int, default=20000,
                   help="scan at most this many stream rows (reservoir cap)")
    h.set_defaults(fn=cmd_hf)

    n = sub.add_parser("noise", help="pointer to mix_noise.py")
    n.set_defaults(fn=cmd_noise)
    return p


def main() -> None:
    args = build_parser().parse_args()
    args.fn(args)


if __name__ == "__main__":
    main()
