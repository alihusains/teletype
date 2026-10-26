#!/usr/bin/env python3
"""Build the two-speaker eval sets: a second *human* talking over the target.

Why this set exists: every existing "noise" tier in the corpus is LibriSpeech
speech plus DEMAND/MUSAN noise, so no set on disk has an informational masker
- a second talker whose own words compete with the dictation. That makes two
claims untestable today:

  * Fluid Speech Mini's pitch ("excels when others are talking nearby"), and
  * any target-speaker-extraction spike (Alkove, diarisation) that is supposed
    to help in exactly that condition.

Both need the same artifact, so it is built once, here.

Pairs are drawn so the two talkers are never the same LibriSpeech speaker:
the subset manifests carry no speaker id, so speaker identity is recovered by
matching each subset clip's reference against the raw `librispeech-*.jsonl`
rows written by `fetch_datasets.sh` (same normalisation as `subset.py`:
lowercase, punctuation stripped). If that map is missing or a reference does
not resolve, this script stops instead of guessing - a same-speaker pair would
silently produce an easy, misleading set.

Outputs (into the dataset cache, idempotent unless --force):

  competing--5db/   the other talker is 5 dB *louder* than the person talking
  competing-0db/    both talkers at the same level
  competing-5db/    the other talker is 5 dB quieter (a colleague across a desk)
  competing-10db/   the other talker is 10 dB quieter (a colleague across a room)

Levels use the same SNR convention as the `noise-{snr}db` sets: the number is
the interferer relative to the target, so negative = interferer louder. Three
tiers, because a single level cannot separate "robust" from "collapsed": a
model that is genuinely better with competing speech should lose less at
0 dB and +5 dB, and both should keep most of the +10 dB result rather than
falling off a cliff at one arbitrary operating point. The -5 dB tier is the
harsh control: a single-channel model is expected to transcribe whichever
talker is louder, and that expectation should be visible rather than assumed.

Stdlib only; shells out to scripts/eval/mix_speech.py. The scoring reference
stays the target transcript: the interferer's words are never part of it.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import random
import re
import struct
import subprocess
import sys
import wave
from datetime import datetime, timezone

CACHE = os.environ.get("TELETYPE_EVAL_CACHE", os.path.expanduser("~/.cache/teletype-eval"))
SEED = int(os.environ.get("EVAL_SEED", "20261026"))
MIXER = os.path.join(os.path.dirname(os.path.abspath(__file__)), "mix_speech.py")

TARGET_SET = "librispeech-clean"
POOL_SET = "librispeech-other"
TIERS = (-5, 0, 5, 10)


def die(msg: str) -> "NoReturn":
    print(f"build_competing_set.py: error: {msg}", file=sys.stderr)
    sys.exit(1)


def norm(text: str) -> str:
    """Reference normalisation, matching subset.py's lowercase/no-punct form."""
    return " ".join(re.sub(r"[^a-z0-9 ]", " ", text.lower()).split())


def speaker_of(audio: str) -> str | None:
    """LibriSpeech paths are `<root>/<split>/<speaker>/<chapter>/<utt>.<ext>`."""
    parts = audio.replace("\\", "/").split("/")
    if "LibriSpeech" in parts:
        i = parts.index("LibriSpeech")
        if len(parts) > i + 2:
            return parts[i + 2]
    return None


def load_speakers(raw_jsonl: str) -> dict[str, str]:
    """Map normalised reference -> LibriSpeech speaker id."""
    if not os.path.exists(raw_jsonl):
        die(f"missing {raw_jsonl}; it is what proves the two talkers differ "
            "(run scripts/eval/fetch_datasets.sh for this split)")
    by_ref: dict[str, str] = {}
    with open(raw_jsonl) as f:
        for line in f:
            if not line.strip():
                continue
            row = json.loads(line)
            spk = speaker_of(row.get("audio", ""))
            if spk:
                by_ref.setdefault(norm(row.get("reference", "")), spk)
    if not by_ref:
        die(f"{raw_jsonl}: no rows resolved to a LibriSpeech speaker")
    return by_ref


def duration(path: str) -> float:
    with wave.open(path, "rb") as w:
        return w.getnframes() / float(w.getframerate())


def rms_of(path: str) -> float:
    with wave.open(path, "rb") as w:
        raw = w.readframes(w.getnframes())
    n = len(raw) // 2
    if n == 0:
        return 0.0
    vals = struct.unpack(f"<{n}h", raw)
    return math.sqrt(sum((v / 32768.0) ** 2 for v in vals) / n)





def load_rows(set_name: str, speakers: dict[str, str]) -> list[dict]:
    set_dir = os.path.join(CACHE, set_name)
    manifest = os.path.join(set_dir, "manifest.jsonl")
    if not os.path.exists(manifest):
        die(f"missing {manifest} (run scripts/eval/fetch_datasets.sh)")
    rows = []
    with open(manifest) as f:
        for line in f:
            if not line.strip():
                continue
            row = json.loads(line)
            audio = row["audio"]
            if not os.path.isabs(audio):
                audio = os.path.join(set_dir, audio)
            spk = speakers.get(norm(row["reference"]))
            if spk is None:
                die(f"{set_name}: cannot resolve the speaker of {row['id']!r} "
                    "from the raw LibriSpeech jsonl; refusing to pair it blind")
            row["_audio"] = audio
            row["_speaker"] = spk
            row["_seconds"] = duration(audio)
            rows.append(row)
    if not rows:
        die(f"{manifest} has no rows")
    return rows


def pair(target: dict, pool: list[dict], rng: random.Random) -> dict:
    """Pick an interferer from a different speaker, preferring one long enough
    to cover the target without looping."""
    others = [r for r in pool if r["_speaker"] != target["_speaker"]]
    if not others:
        die(f"no interferer from a different speaker than {target['_speaker']}")
    long_enough = [r for r in others if r["_seconds"] >= target["_seconds"]]
    return rng.choice(long_enough or others)


def build(tier: int, pairs: list[dict], force: bool) -> int:
    # `competing--5db` for -5, matching the `noise--5db` naming.
    name = f"competing-{tier}db"
    out_dir = os.path.join(CACHE, name)
    if os.path.exists(os.path.join(out_dir, ".done")) and not force:
        print(f"have {name} (use --force to rebuild)")
        return 0
    os.makedirs(os.path.join(out_dir, "audio"), exist_ok=True)
    manifest_path = os.path.join(out_dir, "manifest.jsonl")
    levels: list[float] = []
    deltas: list[float] = []
    scaled = 0
    with open(manifest_path, "w") as f:
        for i, p in enumerate(pairs):
            cid = f"{name}-{i + 1:04d}"
            dest = os.path.join(out_dir, "audio", cid + ".wav")
            subprocess.run(
                [sys.executable, MIXER, "--target", p["target_audio"],
                 "--interferer", p["interferer_audio"], "--snr-db", str(tier),
                 "--delay-ms", str(p["delay_ms"]), "--out", dest,
                 "--seed", str(p["seed"])],
                check=True,
                stdout=subprocess.DEVNULL,
            )
            with open(dest + ".json") as mf:
                mix = json.load(mf)
            # The mixer reports the ratio it actually produced, from the same
            # speech-active metric it used to set the gain. A mismatch means
            # the gain maths and the measurement disagree, which inverts the
            # dose-response while still producing plausible WERs.
            if abs(mix["measured_speech_ratio_db"] - tier) > 0.5:
                die(f"{cid}: mixer reports {mix['measured_speech_ratio_db']:+.2f} dB "
                    f"for a {tier:+d} dB request")
            levels.append(mix["measured_speech_ratio_db"])
            if mix["clip_scale"] < 1.0:
                scaled += 1
            # Informational: the whole-file level change is not expected to
            # match any formula, because it depends on how gappy both clips
            # are. It is printed so a set that is 10 dB quieter than the clean
            # baseline cannot go unnoticed.
            deltas.append(20.0 * math.log10(rms_of(dest) / rms_of(p["target_audio"])))
            f.write(json.dumps({
                "id": cid,
                "audio": f"audio/{cid}.wav",
                "reference": p["reference"],
                "tags": ["competing-speaker", "librispeech", f"ssr-{tier}db"],
                "human": False,
            }) + "\n")
    with open(os.path.join(out_dir, ".done"), "w") as f:
        f.write(datetime.now(timezone.utc).strftime("%FT%TZ") + "\n")

    levels.sort()
    deltas.sort()
    print(f"mixed {len(pairs)} clips with a second talker at {tier} dB -> {out_dir}")
    print(f"  speech-active ratio: median {levels[len(levels) // 2]:+.2f} dB "
          f"(requested {tier:+d})")
    print(f"  whole-file level vs clean target: median {deltas[len(deltas) // 2]:+.2f} dB "
          f"[{deltas[0]:+.2f}, {deltas[-1]:+.2f}]; {scaled} clip(s) had to be scaled down")
    return len(pairs)


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--n", type=int, default=200,
                   help="how many target clips to pair (default: all 200)")
    p.add_argument("--tiers", default="0,5,10",
                   help="interferer levels in dB, comma separated (default -5,0,5,10; negative = interferer louder)")
    p.add_argument("--force", action="store_true", help="rebuild sets that already exist")
    args = p.parse_args()

    speakers = load_speakers(os.path.join(CACHE, f"{TARGET_SET}.jsonl"))
    speakers.update(load_speakers(os.path.join(CACHE, f"{POOL_SET}.jsonl")))
    targets = load_rows(TARGET_SET, speakers)
    pool = load_rows(POOL_SET, speakers)

    rng = random.Random(SEED)
    picked = sorted(rng.sample(targets, min(args.n, len(targets))), key=lambda r: r["id"])
    pairs = []
    for target in picked:
        other = pair(target, pool, rng)
        pairs.append({
            "target_id": target["id"],
            "target_audio": target["_audio"],
            "target_speaker": target["_speaker"],
            "target_seconds": round(target["_seconds"], 2),
            "interferer_id": other["id"],
            "interferer_audio": other["_audio"],
            "interferer_speaker": other["_speaker"],
            "interferer_seconds": round(other["_seconds"], 2),
            "looped": other["_seconds"] < target["_seconds"],
            "delay_ms": rng.choice([200, 300, 400, 500, 600]),
            "seed": rng.randrange(1_000_000),
            "reference": target["reference"],
        })

    if any(x["target_speaker"] == x["interferer_speaker"] for x in pairs):
        die("internal error: a same-speaker pair slipped through")

    tiers = [int(t) for t in args.tiers.split(",") if t.strip()]
    total = 0
    for tier in tiers:
        total += build(tier, pairs, args.force)

    provenance = {
        "seed": SEED,
        "target_set": TARGET_SET,
        "pool_set": POOL_SET,
        "tiers_db": tiers,
        "pairs": len(pairs),
        "distinct_target_speakers": len({x["target_speaker"] for x in pairs}),
        "distinct_interferer_speakers": len({x["interferer_speaker"] for x in pairs}),
        "looped_interferers": sum(1 for x in pairs if x["looped"]),
        "mean_target_seconds": round(sum(x["target_seconds"] for x in pairs) / len(pairs), 2),
        "pairing": pairs,
    }
    prov_path = os.path.join(CACHE, "competing-build.json")
    with open(prov_path, "w") as f:
        json.dump(provenance, f, indent=2)
        f.write("\n")
    print(f"provenance -> {prov_path}")
    print(f"{len(pairs)} pairs; {provenance['distinct_target_speakers']} target / "
          f"{provenance['distinct_interferer_speakers']} interferer speakers; "
          f"{provenance['looped_interferers']} interferers had to be looped")
    if total == 0:
        sys.exit("nothing built")


if __name__ == "__main__":
    main()
