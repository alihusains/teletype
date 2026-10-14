# Eval results (auto-generated table; every phase appends a row)

Gate targets (plan §0.2): 8-set verbatim-WER blend ≤ 8.0%, vocab recall ≥
90% (false alarms ≤ 1%), release→paste p95 ≤ 700 ms with polish on.

| Date | Git SHA | Engine | Polish | Sets | Verbatim WER (micro) | Clean WER human | Vocab recall / FA | Entity acc | R2P p50/p95/p99 (ms) | Notes |
|------|---------|--------|--------|------|----------------------|-----------------|-------------------|------------|----------------------|-------|
| 2026-10-14 | n/a (harness smoke) | none (replay) | n/a | synthetic-17 (`evals/corpus`) | 0.0184 | n/a | n/a | 1.0000 | n/a | Smoke only: `say`-synthesized plumbing check, NOT a baseline. Legacy WER 0.0296/0.0428 (micro/macro), polish mean 91.2. |
| Gate 0 baseline | — | ggml Parakeet v3 | S1-mini | all 8 sets | — | — | — | — | — | BLOCKED (see below). |

## Gate 0 baseline: blocked (2026-10-14)

Attempted by the P0.4 worker, time-boxed per plan §0.1 rule 7. What ran vs
what is blocked:

- **Ran:** full eval infra + replay verification on the 17-clip synthetic
  corpus (row above); `fetch_datasets.sh` syntax/shellcheck clean, `subset.py`
  sampling deterministic + verified, `mix_noise.py` verified at exactly the
  requested SNR (10.00/5.00 dB on synthetic tones), `record_clip.sh`
  validated (id rejection exit 1, meta schema verified).
- **Blocked — datasets (GBs, not downloaded):** `~/.cache/teletype-eval/`
  does not exist; nothing fetched. LibriSpeech (OpenSLR 12) and MUSAN
  (OpenSLR 17) URLs verified reachable (HTTP 200); FLEURS + Earnings-22
  chunked repos verified public on the Hub; AMI has no reliable Hub mirror
  (manual path documented in the fetch script). Disk is 96% full (17 Gi
  free) — a full fetch (~5–8 GB with models) needs coordination, not a
  background download.
- **Blocked — models:** no Parakeet/Whisper model on disk; catalog URL
  (`ggml-org/parakeet-GGUF`, MIT base) verified reachable. S1-mini polish
  needs the llama-server sidecar + weights (outside the harness).
- **Blocked — live latency:** `--live` requires DictationTrace JSONL from
  the real pipeline; the P0.2 trace struct landed (field names match the
  plan) but no dictation→harness wiring exists yet, so there are no traces
  to score. Replay latency does not count toward the gate.
- **Blocked — human corpus:** 0 clips recorded (needs a human + mic +
  consent; `record_clip.sh` + `FORMAT.md` ready).

Next step for whoever unblocks this: free disk → run
`scripts/eval/fetch_datasets.sh` → record ≥ 120 clips → run
`teletype-eval run --engine parakeet-ultra --polish s1-mini --set
human,librispeech,earnings22-vocab,ami,cv,noise --live --out
evals/runs/<git-sha>/` → append the row here.
