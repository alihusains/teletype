# Teletype Eval Report

- **Git SHA:** 19b2767
- **Timestamp:** 1793033971
- **Model:** fluid-speech-mini
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0751 |
| Macro WER | 0.1200 |
| Mean polish score | 43.6 |
| ASR p50 / p95 (ms) | 268 / 1147 |
| Total p50 / p95 (ms) | 268 / 1147 |
| Model load (ms) | 234 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| cv | 200 | 7349 | 552 | 0.0751 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| cv-0139 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0145 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0150 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0118 | 0.7500 | 2 | 0 | 1 | my earliest memories was... | My earliest memory is Naz. |
| cv-0117 | 0.6667 | 2 | 0 | 0 | yes  hear that | Yes, she does. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 125 |
| no_hallucination | 116 |
| no_fillers | 94 |
| bullets_when_listing | 4 |
| sentence_cased | 4 |
| terminal_punctuation | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 197 | 83 | 2924 | 423 | 268 | 1147 | 2155 |
| total | 197 | 83 | 2924 | 423 | 268 | 1147 | 2155 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0557 |
| Verbatim WER (macro) | 0.0855 |
| Entity accuracy | 0.8235 |

