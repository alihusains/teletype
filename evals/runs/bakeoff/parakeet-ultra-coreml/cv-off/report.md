# Teletype Eval Report

- **Git SHA:** 00f8a76
- **Timestamp:** 1792361750
- **Model:** parakeet-ultra-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0837 |
| Macro WER | 0.1265 |
| Mean polish score | 42.3 |
| ASR p50 / p95 (ms) | 57 / 207 |
| Total p50 / p95 (ms) | 57 / 207 |
| Model load (ms) | 8781 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| cv | 200 | 7349 | 615 | 0.0837 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| cv-0118 | 1.0000 | 3 | 0 | 1 | my earliest memories was... | My artist memory is nuts. |
| cv-0139 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0145 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0150 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0117 | 0.6667 | 2 | 0 | 0 | yes  hear that | Yes, she does. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 130 |
| no_hallucination | 115 |
| no_fillers | 85 |
| terminal_punctuation | 41 |
| sentence_cased | 8 |
| bullets_when_listing | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 31 | 432 | 88 | 57 | 207 | 306 |
| total | 200 | 31 | 432 | 88 | 57 | 207 | 306 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0589 |
| Verbatim WER (macro) | 0.0891 |
| Entity accuracy | 0.8235 |

