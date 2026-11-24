# Teletype Eval Report

- **Git SHA:** 00f8a76
- **Timestamp:** 1792361428
- **Model:** sherpa-parakeet-v2-int8
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0511 |
| Macro WER | 0.0678 |
| Mean polish score | 59.5 |
| ASR p50 / p95 (ms) | 290 / 956 |
| Total p50 / p95 (ms) | 290 / 956 |
| Model load (ms) | 562 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 84 | 0.0511 |
| snr-5db | 100 | 1644 | 84 | 0.0511 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-5db-0010 | 0.4286 | 2 | 0 | 1 | asked zarathustra vehemently what doest thou here | answered to Isra vehemently. What doest thou here? |
| noise-5db-0025 | 0.4286 | 3 | 0 | 0 | the vil stuck fast in his throat | The veil stuck fast in the circle. |
| noise-5db-0033 | 0.4286 | 1 | 2 | 0 | but you are a jolly fellow too | But you're a jolly fellow. |
| noise-5db-0019 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | Where are you bound for, young man? |
| noise-5db-0037 | 0.3125 | 5 | 0 | 0 | will yo' alls please speak a lil louder he holler down the c… | Will you all please speak a little louder? he hollered down … |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 45 |
| no_hallucination | 43 |
| terminal_punctuation | 15 |
| sentence_cased | 13 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 125 | 1698 | 410 | 290 | 956 | 1283 |
| total | 100 | 125 | 1698 | 410 | 290 | 956 | 1283 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0495 |
| Verbatim WER (macro) | 0.0646 |

