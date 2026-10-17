# Eval results (auto-generated table; every phase appends a row)

Gate targets (plan §0.2): 8-set verbatim-WER blend ≤ 8.0%, vocab recall ≥
90% (false alarms ≤ 1%), release→paste p95 ≤ 700 ms with polish on.

| Date | Git SHA | Engine | Polish | Sets | Verbatim WER (micro) | Clean WER human | Vocab recall / FA | Entity acc | R2P p50/p95/p99 (ms) | Notes |
|------|---------|--------|--------|------|----------------------|-----------------|-------------------|------------|----------------------|-------|
| 2026-10-14 | n/a (harness smoke) | none (replay) | n/a | synthetic-17 (`evals/corpus`) | 0.0184 | n/a | n/a | 1.0000 | n/a | Smoke only: `say`-synthesized plumbing check, NOT a baseline. Legacy WER 0.0296/0.0428 (micro/macro), polish mean 91.2. |
| 2026-10-15 | `8197681` | ggml Parakeet v3 (ggml-parakeet-tdt-0.6b-v3-q8_0) | S1-mini (recorded; harness scores raw ASR) | 16 on-disk sets (all except `cv`/`human`) | 0.0789 | n/a (human set empty) | 0.8277 / 0.0000 | 0.8097 | n/a (live traces pending) | **Gate 0 baseline, measured.** 1306 cases, 14m13s, `evals/runs/baseline/`. Harness asr p50/p95/p99 = 433/782/1010 ms (NOT release→paste). 1 case_error (ami-0019). Per-set table below. |
| 2026-10-17 | `98f116a` | ggml Parakeet v3 q8_0 (baseline model file; `--engine parakeet-ultra` label kept for comparability) | n/a | `noise--5db` (100) + `libri-noise-{10,5,0,-5}db` (200 each) | 0.0894 (all 5 sets; per-SNR below) | n/a | n/a | n/a | n/a | **P5.8 noise-mode WER baseline (noise handling OFF).** -5 dB added to the baseline noise series; monotonic per SNR on both sources. 2 "No speech detected" case_errors. `evals/runs/p58-dfn3-*-2026-10-17*/`. |

## Gate 0 baseline: measured (2026-10-15)

Full-corpus run (`--engine parakeet-ultra --polish s1-mini`, every on-disk
set, `--out evals/runs/baseline`): **1306 cases, 14m13s wall, 1 case_error**
(`ami-0019`: "No speech detected" on a real clip; scored as a total error and
listed in `report.json`'s `case_errors`).

**Caveat (kept from the report header):** every shipped set except the human
corpus is machine/broadcast audio (`human: false`). These numbers validate
harness + engine plumbing and give the relative signal the gates compare;
they are **not** a statement about human-speech accuracy. The human set is
still 0/120 — the `Clean WER human` column cannot be filled until someone
records.

### Per-set verbatim WER (micro)

| Set | Cases | Ref words | Verbatim WER |
|---|---|---:|---:|
| librispeech (clean+other) | 400 | 7,631 | **0.0274** |
| earnings22-vocab (whole calls) | 6 | 15,814 | **0.1336** |
| ami | 100 | 761 | **0.1222** |
| noise · 10 dB | 100 | 1,644 | 0.0505 |
| noise · 5 dB | 100 | 1,644 | 0.0590 |
| noise · 0 dB | 100 | 1,644 | 0.0785 |
| fleurs-it | 50 | 1,206 | 0.0299 |
| fleurs-es | 50 | 1,196 | 0.0535 |
| fleurs-fr | 50 | 1,237 | 0.0639 |
| fleurs-en | 50 | 1,095 | 0.0648 |
| fleurs-de | 50 | 1,011 | 0.0673 |
| fleurs-pt | 50 | 1,158 | 0.0699 |
| fleurs-uk | 50 | 949 | 0.0748 |
| fleurs-ru | 50 | 973 | 0.0761 |
| fleurs-pl | 50 | 961 | 0.0874 |
| fleurs-nl | 50 | 1,058 | 0.0926 |

### P5.8 noise-mode WER baseline (2026-10-17, noise handling OFF)

Same model file as the Gate 0 baseline (ggml Parakeet v3 q8_0; the current app
default `parakeet-tdt-v3` resolves to the same
`ggml-parakeet-tdt-0.6b-v3-q8_0.bin`), so the rows are directly comparable.
No DFN3 denoise in this run (weights + auto-enable are a later unit).

| Set | Cases | Ref words | Verbatim WER (micro) |
|---|---|---:|---:|
| noise · -5 dB (say-synth; same 100 clips as the 10/5/0 dB series) | 100 | 1,644 | **0.1387** |
| LibriSpeech test-other · 10 dB (MUSAN mix) | 200 | 3,505 | 0.0516 |
| LibriSpeech test-other · 5 dB | 200 | 3,505 | 0.0588 |
| LibriSpeech test-other · 0 dB | 200 | 3,505 | 0.0981 |
| LibriSpeech test-other · -5 dB | 200 | 3,505 | **0.1612** |

Baseline noise series extended to -5 dB: WER is monotonic in SNR on both
sources (say-synth 0.0505/0.0590/0.0785/**0.1387**; test-other
0.0516/0.0588/0.0981/**0.1612**). Mixes: `scripts/eval/mix_noise.py`, MUSAN
pool seed 20261014, all 200 test-other clips x 4 SNRs (800 clips, 2m13s).
2/800 "No speech detected" case_errors (scored as total errors, same
convention as `ami-0019`).

Reproduce:
```
cargo build --release -p teletype-eval
./target/release/teletype-eval run --engine parakeet-ultra \
  --model "$HOME/Library/Application Support/com.teletype.app/models/ggml-parakeet-tdt-0.6b-v3-q8_0.bin" \
  --set noise--5db --out evals/runs/p58-dfn3-noise-2026-10-17-minus5db/
./target/release/teletype-eval run --engine parakeet-ultra \
  --model "$HOME/Library/Application Support/com.teletype.app/models/ggml-parakeet-tdt-0.6b-v3-q8_0.bin" \
  --set libri-noise-10db,libri-noise-5db,libri-noise-0db,libri-noise--5db \
  --out evals/runs/p58-dfn3-libri-noise-2026-10-17/
```

Notes that matter for reading the numbers:

- **earnings22-vocab** is scored per *call*, not per VAD chunk (the chunked
  repo ships no per-chunk transcripts; the old proportional split fabricated
  WER — 0.82 → 0.1336 after the rebuild, same model and audio). See commit
  `429e860` and learning L025.
- **Noise SNRs** were independently verified from the mixed files (0/5/10 dB
  whole-file RMS, within ±0.5 dB; the few louder 0 dB cases are the mixer's
  documented clip-protection scaling). MUSAN is extracted as the noise
  category only.
- **Worst cases** are one-to-three-word AMI clips where a single insertion
  scores > 1.0 (e.g. `ami-0029`: "and maybe" → "And bearing in mind that");
  they do not move the micro average (93 errors in 761 words overall).

### Extended metrics

| Metric | Value | Note |
|---|---|---|
| Verbatim WER (micro / macro) | 0.0789 / 0.0620 | The 8-set blend gate (≤ 8.0 %) is met on the on-disk sets — **human set still empty**, so this is provisional. |
| Vocab recall / false alarms | 0.8277 / 0.0000 | earnings22-vocab, 6 calls. The ≥ 90 % gate is a **Phase 3** target (native boosting P3-A or rescoring P3-G). |
| Entity accuracy | 0.8097 | |
| Polish mean | 60.7 | Recorded-only: the harness scores raw ASR; wiring the real S1-mini polish pass is an open decision (post-`1afbf8f`). |

### Harness latency (NOT release→paste)

asr / total per-case p50/p95/p99 = **433 / 782 / 1010 ms** over 1305 cases —
one model load at start, including the 21-minute calls (max 21.9 s). This is
the harness's own timing, not the app's release→paste. The gate's p50/p95/p99
requires live `DictationTrace` lines from a real dictation session (traces
auto-write to `<app_data>/traces/YYYY-MM-DD.jsonl`; then
`teletype-eval run --live --traces <jsonl>`).

### Still open before this checkbox is fully closed

- **Live p50/p95/p99** — needs one real dictation session with the app
  (human + mic); trace plumbing and `--live` scoring are ready and verified.
- **CV set** — `mozilla-foundation/common_voice_17_0` is access-gated
  (`HF_TOKEN` + accepted terms — human action).
- **Human corpus** — 0/120 clips (`scripts/eval/record_clip.sh` ready).
- **CI green** — the branch had not been pushed when this ran.

Reproduce: `bash scripts/eval/fetch_datasets.sh` → the `teletype-eval run`
command above; the markdown + JSON reports are committed in
`evals/runs/baseline/`.
