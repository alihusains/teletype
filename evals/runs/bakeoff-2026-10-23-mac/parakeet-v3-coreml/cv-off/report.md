# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792751883
- **Model:** parakeet-v3-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0801 |
| Macro WER | 0.1315 |
| Mean polish score | 42.8 |
| ASR p50 / p95 (ms) | 56 / 208 |
| Total p50 / p95 (ms) | 56 / 208 |
| Model load (ms) | 94 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| cv | 200 | 7349 | 589 | 0.0801 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| cv-0118 | 1.2500 | 3 | 0 | 2 | my earliest memories was... | My artist of memory is Nas. |
| cv-0139 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0145 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0150 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0175 | 1.0000 | 2 | 0 | 3 | what sports is your favorite? | What the sports is you aware about it. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 127 |
| no_hallucination | 116 |
| no_fillers | 96 |
| terminal_punctuation | 26 |
| bullets_when_listing | 4 |
| sentence_cased | 3 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 31 | 543 | 90 | 56 | 208 | 260 |
| total | 200 | 31 | 543 | 90 | 56 | 208 | 260 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0602 |
| Verbatim WER (macro) | 0.0979 |
| Entity accuracy | 0.8235 |

