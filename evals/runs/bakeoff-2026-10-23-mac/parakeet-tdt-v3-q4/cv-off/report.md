# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792751986
- **Model:** parakeet-tdt-v3-q4
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.1244 |
| Macro WER | 0.1443 |
| Mean polish score | 42.6 |
| ASR p50 / p95 (ms) | 208 / 701 |
| Total p50 / p95 (ms) | 208 / 701 |
| Model load (ms) | 150 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| cv | 200 | 7349 | 914 | 0.1244 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| cv-0139 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0145 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0150 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0155 | 1.0000 | 3 | 0 | 1 | do you buy cloth-thes? | So you'll buy clothes, sis. |
| cv-0190 | 0.8000 | 3 | 0 | 1 | i'll be running so fast. | I will run in so fast. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 130 |
| no_hallucination | 115 |
| no_fillers | 87 |
| terminal_punctuation | 8 |
| bullets_when_listing | 4 |
| sentence_cased | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 197 | 62 | 1321 | 275 | 208 | 701 | 1086 |
| total | 197 | 62 | 1321 | 275 | 208 | 701 | 1086 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1004 |
| Verbatim WER (macro) | 0.1030 |
| Entity accuracy | 0.7059 |

