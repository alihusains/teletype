# Teletype Eval Harness

Measures the three things the "full Wispr Flow parity" claim actually rests
on: ASR accuracy (WER), text-polish quality, and latency. Nothing in this
directory is a product claim by itself — the report says what it is.

## What each metric means

- **Micro WER** — total word errors (substitutions + deletions + insertions)
  over total reference words, summed across all cases. This is the headline
  ASR-accuracy number.
- **Macro WER** — the mean of the per-case WERs. A single terrible case
  moves macro WER but barely moves micro WER; both are reported so neither
  can hide the other.
- **Polish score (0-100)** — a deterministic rubric over the polished text:
  `no_content_loss` (50) and `no_hallucination` (40) dominate; `no_fillers`
  (4), `sentence_cased` (3), `terminal_punctuation` (2), and
  `no_markdown_fences` (1) are surface polish. `bullets_when_listing` is
  reported but unscored. A score of 100 means every check passed.
- **p50 / p95 latency** — nearest-rank percentiles over per-case wall-clock
  milliseconds. Two series: `asr` (transcribe call only) and `total`
  (ASR + polish). Model load time is reported separately, once, because it
  is a one-off.

**WER over `say`-synthesised audio validates the harness and is not a
statement about Teletype's accuracy on human speech.**

## Replay mode (CI path)

Replay mode reads pre-recorded ASR output per case id and runs WER, polish
scoring, latency percentiles, the report, and the threshold gate with no
model, no network, and no download. It runs in under a couple of seconds.

```bash
# 1. Generate the starter corpus (macOS `say`, 18 utterances).
bash evals/generate-corpus.sh

# 2. Record hypotheses for each case id. One JSON object per line:
#    {"id":"plain-01","hypothesis":"the deploy went out this morning and it is all good",
#     "polished":"The deploy went out this morning, and it is all good.",
#     "asr_ms":312.5,"total_ms":1800.0}
#    `polished` may be omitted (defaults to `hypothesis`).

# 3. Run the harness.
cargo run -p teletype-eval --bin eval -- \
    --corpus evals/corpus \
    --hypotheses evals/hypotheses.jsonl \
    --report evals/report.md \
    --json evals/report.json \
    --thresholds evals/thresholds.json
```

The driver exits `1` on any threshold breach and prints exactly which metric
breached. A missing or malformed thresholds file is an error, not a pass.

## Measure mode (real run)

Measure mode loads the real model through `teletype-speech`, decodes each
case's wav, and times the `transcribe` call per case. This is what produces
numbers you can quote.

```bash
cargo run -p teletype-eval --bin eval -- \
    --corpus evals/corpus \
    --asr parakeet \
    --model /path/to/model.bin \
    --report evals/report.md \
    --json evals/report.json \
    --thresholds evals/thresholds.json
```

Audio is decoded as 16 kHz mono. If a case is not 16 kHz mono the driver
warns and resamples with the same resampler the live dictation path uses;
the report should then be read with that caveat.

## Competing-speaker sets (`competing-{snr}db`)

Every `noise-*` tier is LibriSpeech speech plus DEMAND/MUSAN noise, which is a
*stationary* masker. Nothing in that family puts a second **talker** over the
dictation, so claims like "excels when others are talking nearby" and any
target-speaker-extraction idea are untestable with those sets.

`scripts/eval/build_competing_set.py` builds the missing artefact from data
already in the cache: each `librispeech-clean` clip is mixed with a clip from
a *different* LibriSpeech speaker (`librispeech-other`), delayed 200-600 ms,
at three levels. Negative SNR means the interferer is louder, matching the
`noise-*` convention:

| Set | Second talker |
|---|---|
| `competing-0db` | equally loud |
| `competing-5db` | 5 dB quieter than the person dictating |
| `competing-10db` | 10 dB quieter (a colleague across a room) |

A `--5db` tier (interferer *louder*) was built and discarded: a single-channel
model transcribes whichever talker is louder, so it measures the mixer, not
the engine (see `.agents/brain/learnings/2026-10-26-competing-speaker-mix-level-metric.md`).

```bash
python3 scripts/eval/build_competing_set.py           # builds all three tiers
python3 scripts/eval/build_competing_set.py --force   # rebuild after a change
```

The scorer still sees only the target transcript. Speaker identity is
recovered from the raw `librispeech-*.jsonl` rows, and the builder refuses to
pair a clip it cannot resolve, because a same-speaker pair would silently make
the set easy. It also verifies the realised mix level against the requested
ratio and aborts if the two disagree by more than 1.5 dB.

Every pair (target id/speaker, interferer id/speaker, delay, loop offset) is
recorded in `~/.cache/teletype-eval/competing-build.json`, so a run can be
audited without re-deriving the RNG stream.

## Adding real human recordings

Same directory layout as the generated corpus:

1. Drop a `.wav` into `evals/corpus/audio/` (16 kHz mono recommended).
2. Add a line to `evals/corpus/manifest.jsonl` with `"human": true` and the
   exact reference text as spoken (lowercase, no trailing period).
3. Re-run replay or measure mode. The report will show the human case count
   and, only if *all* cases are human, drop the synthesised-audio warning.

The `human` flag is what governs how the numbers may be quoted: a WER
computed over synthesised speech is a plumbing check, not a product claim.
