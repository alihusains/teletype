# Teletype Eval Report

- **Git SHA:** cee77e6
- **Timestamp:** 1792625991
- **Model:** parakeet-tdt-v3
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0840 |
| Macro WER | 0.1285 |
| Mean polish score | 43.4 |
| ASR p50 / p95 (ms) | 303 / 1649 |
| Total p50 / p95 (ms) | 303 / 1649 |
| Model load (ms) | 258 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| cv | 200 | 7349 | 617 | 0.0840 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| cv-0118 | 1.0000 | 3 | 0 | 1 | my earliest memories was... | My artist memory is nurs. |
| cv-0139 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0145 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0150 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0175 | 1.0000 | 2 | 0 | 3 | what sports is your favorite? | What the sports is you aware about it. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 126 |
| no_hallucination | 116 |
| no_fillers | 91 |
| terminal_punctuation | 7 |
| bullets_when_listing | 4 |
| sentence_cased | 3 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 197 | 69 | 6214 | 519 | 303 | 1649 | 3866 |
| total | 197 | 69 | 6214 | 519 | 303 | 1649 | 3866 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0603 |
| Verbatim WER (macro) | 0.0919 |
| Entity accuracy | 0.8235 |

