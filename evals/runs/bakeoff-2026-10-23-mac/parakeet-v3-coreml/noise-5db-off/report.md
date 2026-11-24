# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792751911
- **Model:** parakeet-v3-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0651 |
| Macro WER | 0.0838 |
| Mean polish score | 61.1 |
| ASR p50 / p95 (ms) | 43 / 100 |
| Total p50 / p95 (ms) | 43 / 100 |
| Model load (ms) | 97 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 107 | 0.0651 |
| snr-5db | 100 | 1644 | 107 | 0.0651 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-5db-0008 | 1.0000 | 0 | 0 | 3 | and also these | And also these are the two. |
| noise-5db-0010 | 0.4286 | 3 | 0 | 0 | asked zarathustra vehemently what doest thou here | Aja Twistra vehemently, what doest thou hear? |
| noise-5db-0025 | 0.4286 | 3 | 0 | 0 | the vil stuck fast in his throat | The veal stuck fast in the circle. |
| noise-5db-0033 | 0.4286 | 2 | 1 | 0 | but you are a jolly fellow too | But you're a jolly fellow chair. |
| noise-5db-0019 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | Where are you bound for, young man? |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 43 |
| no_hallucination | 42 |
| sentence_cased | 10 |
| terminal_punctuation | 7 |
| no_fillers | 3 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 35 | 114 | 49 | 43 | 100 | 111 |
| total | 100 | 35 | 114 | 49 | 43 | 100 | 111 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0627 |
| Verbatim WER (macro) | 0.0804 |

