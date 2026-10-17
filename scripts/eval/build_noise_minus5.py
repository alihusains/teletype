#!/usr/bin/env python3
"""Generate the plan §5.8 noise eval sets: the -5 dB tier and noise mixes.

Builds, idempotently, into the dataset cache (~/.cache/teletype-eval):

  noise--5db/        100 LibriSpeech test-other clips at -5 dB SNR
  noise--5db-human/  human-corpus clips at -5 dB SNR (only if the human set
                     exists and is non-empty in the cache)

The 10/5/0 dB tiers were built by fetch_datasets.sh (whose `noise` target now
also covers -5 dB and the human corpus). This script exists for the -5 dB
rollout because the cache's existing `noise-{10,5,0}db` dirs carry `.done`
markers and fetch_datasets.sh skips them; it reuses the exact RNG stream of
the committed generator (same seed, same MUSAN glob, same per-clip seed
schedule) so the -5 dB sets are the same clips and noise files as the
existing tiers, and the human mixes match what fetch_datasets.sh would
produce.

Stdlib only; shells out to scripts/eval/mix_noise.py.
"""

from __future__ import annotations

import glob
import json
import os
import random
import subprocess
import sys
from datetime import datetime, timezone

CACHE = os.environ.get("TELETYPE_EVAL_CACHE", os.path.expanduser("~/.cache/teletype-eval"))
SEED = int(os.environ.get("EVAL_SEED", "20261014"))
MIXER = os.path.join(os.path.dirname(os.path.abspath(__file__)), "mix_noise.py")
DB = -5


def build(clean_manifest: str, out_dir: str, id_prefix: str, tags: list[str]) -> int:
    musan = sorted(glob.glob(os.path.join(CACHE, "musan", "*", "*", "*.wav")))
    if not musan:
        sys.exit(f"no musan wavs under {CACHE}/musan/*/*/*.wav")
    rows = [json.loads(l) for l in open(clean_manifest) if l.strip()]
    if not rows:
        print(f"skip {os.path.basename(out_dir)}: {clean_manifest} is empty")
        return 0
    rng = random.Random(SEED)
    picked = sorted(rng.sample(rows, min(100, len(rows))), key=lambda r: r["id"])
    os.makedirs(os.path.join(out_dir, "audio"), exist_ok=True)
    manifest_path = os.path.join(out_dir, "manifest.jsonl")
    manifest_dir = os.path.dirname(os.path.abspath(clean_manifest))
    with open(manifest_path, "w") as f:
        for i, row in enumerate(picked):
            cid = f"{id_prefix}-{i + 1:04d}"
            clean = row["audio"]
            if not os.path.isabs(clean):
                clean = os.path.join(manifest_dir, clean)
            dest = os.path.join(out_dir, "audio", cid + ".wav")
            subprocess.run(
                [sys.executable, MIXER, "--clean", clean,
                 "--noise", rng.choice(musan), "--snr-db", str(DB),
                 "--out", dest, "--seed", str(SEED + i)],
                check=True,
            )
            f.write(json.dumps({
                "id": cid,
                "audio": f"audio/{cid}.wav",
                "reference": row.get("reference", row.get("clean", "")),
                "tags": tags + [f"snr-{DB}db"],
                "human": bool(row.get("human", False)),
            }) + "\n")
    # Idempotency marker, matching fetch_datasets.sh convention.
    with open(os.path.join(out_dir, ".done"), "w") as f:
        f.write(datetime.now(timezone.utc).strftime("%FT%TZ") + "\n")
    print(f"mixed {len(picked)} clips at {DB} dB -> {out_dir}")
    return len(picked)


def main() -> None:
    total = 0
    total += build(os.path.join(CACHE, "librispeech-other", "manifest.jsonl"),
                   os.path.join(CACHE, "noise--5db"), "noise--5db", ["noise"])
    human = os.path.join(CACHE, "human", "manifest.jsonl")
    if os.path.exists(human) and os.path.getsize(human) > 0:
        total += build(human, os.path.join(CACHE, "noise--5db-human"),
                       "noise--5db-human", ["noise", "human"])
    else:
        print("skip noise--5db-human: no human set in cache")
    if total == 0:
        sys.exit("nothing built")


if __name__ == "__main__":
    main()
