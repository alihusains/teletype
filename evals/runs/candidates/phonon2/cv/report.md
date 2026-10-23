# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792775364
- **Model:** phonon2
- **Mode:** replay

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0925 |
| Macro WER | 0.1387 |
| Mean polish score | 38.7 |
| ASR p50 / p95 (ms) | 34 / 209 |
| Total p50 / p95 (ms) | 34 / 209 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| cv | 200 | 7349 | 680 | 0.0925 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| cv-0175 | 1.2000 | 5 | 0 | 1 | what sports is your favorite? | But the sport's easier it's a. |
| cv-0139 | 1.0000 | 1 | 0 | 0 | [silence] | Yeah. |
| cv-0145 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0150 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0118 | 0.7500 | 2 | 0 | 1 | my earliest memories was... | My earliest memory is nice. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 135 |
| no_hallucination | 128 |
| no_fillers | 93 |
| terminal_punctuation | 5 |
| bullets_when_listing | 4 |
| sentence_cased | 3 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 0 | 75108 | 452 | 34 | 209 | 357 |
| total | 200 | 0 | 75108 | 452 | 34 | 209 | 357 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0714 |
| Verbatim WER (macro) | 0.1059 |
| Entity accuracy | 0.7059 |

