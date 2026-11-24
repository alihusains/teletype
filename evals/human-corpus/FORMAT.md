# Human corpus format (plan §P0.4)

Real dictations with consent, stored with Git LFS (see `.gitattributes`).
Target: **≥ 120 clips**, ≥ 4 voices, ≥ 3 accents; mics: MacBook, AirPods
(Bluetooth HFP), USB; rooms: quiet + café noise.

Content mix: 25% email/chat, 20% code + technical terms, 15% names and
proper nouns, 15% numbers/dates/money, 15% self-corrections ("no wait, make
that Thursday"), 10% lists.

## Four files per clip

| File              | Contents                                                     |
|-------------------|--------------------------------------------------------------|
| `<id>.wav`        | 16-bit mono WAV, any rate (the harness resamples + warns)    |
| `<id>.verbatim.txt` | What was actually said, fillers included, one line          |
| `<id>.clean.txt`    | What the user wants pasted (fillers/self-corrections fixed) |
| `<id>.meta.json`    | Speaker/mic/noise/context + vocab terms (schema below)      |

`<id>` matches `[a-z0-9-]` (same charset as the harness corpus manifest).

`meta.json` schema (see `example.meta.json`):

```json
{
  "speaker": "js",
  "mic": "macbook",
  "noise": "quiet",
  "app_context": "email",
  "vocab": ["Acme Corp", "ACME"]
}
```

- `speaker`: tag or initials. No real names without written consent; the
  consent note lives with whoever collected the clip, never in this repo.
- `mic`: `macbook` | `airpods` | `usb` | `other`.
- `noise`: `quiet` | `cafe` | `street` | `other`.
- `app_context`: `email` | `chat` | `code` | `notes` | `other`.
- `vocab`: names, tickers, jargon in this clip (used for vocab-recall).

## Recording

```bash
scripts/eval/record_clip.sh <clip-id> [out-dir]   # out-dir defaults here
```

The script records, then prompts for the verbatim/clean lines and the meta
fields. It never overwrites without `FORCE=1`.

## Running the harness on this corpus

`teletype-eval` reads the classic `manifest.jsonl` layout, so bridge the
four-file layout to it (verbatim = reference, clean = clean):

```bash
# generate manifest.jsonl from the four-file layout:
python3 - <<'EOF'
import glob, json, os
d = "evals/human-corpus"
with open(f"{d}/manifest.jsonl", "w") as f:
    for wav in sorted(glob.glob(f"{d}/*.wav")):
        cid = os.path.splitext(os.path.basename(wav))[0]
        verb = open(f"{d}/{cid}.verbatim.txt").read().strip()
        clean = open(f"{d}/{cid}.clean.txt").read().strip()
        meta = json.load(open(f"{d}/{cid}.meta.json"))
        f.write(json.dumps({"id": cid, "audio": f"{cid}.wav",
                            "reference": verb.lower(), "clean": clean.lower(),
                            "tags": [meta["app_context"]], "human": True}) + "\n")
EOF
cargo run -p teletype-eval --bin teletype-eval -- run \
  --engine parakeet-ultra --polish s1-mini --corpus evals/human-corpus \
  --model <model> --live --traces <traces.jsonl> \
  --out evals/runs/<git-sha>/ --thresholds evals/thresholds.json
```

## Status

No clips recorded yet — recording needs a human with a mic (see the
recording README in this directory for the 20-line starter set). The empty
`manifest.jsonl` keeps `--corpus evals/human-corpus` loadable (0 cases)
until the first clips land.
