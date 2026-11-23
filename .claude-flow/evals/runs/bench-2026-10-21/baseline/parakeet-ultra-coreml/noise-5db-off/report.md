# Teletype Eval Report

- **Git SHA:** cee77e6
- **Timestamp:** 1792609539
- **Model:** parakeet-ultra-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0547 |
| Macro WER | 0.0677 |
| Mean polish score | 62.9 |
| ASR p50 / p95 (ms) | 43 / 74 |
| Total p50 / p95 (ms) | 43 / 74 |
| Model load (ms) | 98 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 90 | 0.0547 |
| snr-5db | 100 | 1644 | 90 | 0.0547 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-5db-0025 | 0.5714 | 4 | 0 | 0 | the vil stuck fast in his throat | The veil stuck fast to the syrup. |
| noise-5db-0010 | 0.4286 | 2 | 0 | 1 | asked zarathustra vehemently what doest thou here | asked their Twistra vehemently, What doest thou hear? |
| noise-5db-0033 | 0.4286 | 2 | 1 | 0 | but you are a jolly fellow too | But you're a jolly fellow chip. |
| noise-5db-0019 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | Where are you bound for, young man? |
| noise-5db-0037 | 0.3125 | 5 | 0 | 0 | will yo' alls please speak a lil louder he holler down the c… | Will you all please speak a little louder? he hollered down … |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 41 |
| no_hallucination | 40 |
| sentence_cased | 10 |
| terminal_punctuation | 9 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 33 | 115 | 48 | 43 | 74 | 110 |
| total | 100 | 33 | 115 | 48 | 43 | 74 | 110 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0525 |
| Verbatim WER (macro) | 0.0643 |

