# ASR benchmark — shipping engines vs external candidates

Run root: `evals/runs/bench-2026-10-28`

Every row was scored by the same Rust harness (`teletype-eval`), so the numbers are directly comparable. Boost axis: off for all cells.

## Verbatim WER per set (lower is better)

| engine | ami | competing-0db | cv | fleurs-en | fleurs-pl | librispeech-clean | librispeech-other | noise-0db | pooled | cases |
|---|---|---|---|---|---|---|---|---|---|---|
| `large-v3-turbo-q5` | 0.130 | — | 0.097 | 0.041 | 1.175 | 0.018 | 0.040 | 0.098 | 0.1212 | 900 |
| `parakeet-ultra-coreml` | 0.105 | — | 0.059 | 0.050 | 0.078 | 0.016 | 0.036 | 0.072 | 0.0489 | 900 |
| `lfm3` | 0.113 | 0.351 | 0.105 | 0.042 | 0.113 | 0.019 | 0.064 | 0.128 | 0.1263 | 1100 |

## Summary

| engine | group | pooled legacy WER | verbatim WER | errors/words | ASR p95 (ms) | ASR mean (ms) | RTF | failed cases |
|---|---|---|---|---|---|---|---|---|
| `parakeet-ultra-coreml` | baseline | 0.0614 | 0.0489 | 1194/19441 | 122 | 61 | — | 4 |
| `lfm3` | candidates | 0.1427 | 0.1263 | 3363/23567 | 976 | 444 | 0.0491 | 0 |
| `large-v3-turbo-q5` | baseline | 0.1442 | 0.1212 | 2803/19441 | 954 | 742 | — | 0 |

*pooled legacy WER* = total errors / total reference words summed over the sets; *verbatim WER* = the normalized rate weighted by reference words. Latency is per-clip ASR time on this host (Apple M4 Pro, macOS, Metal for the ggml engines). RTF is only defined for the Python-run candidates (audio seconds / model seconds).
