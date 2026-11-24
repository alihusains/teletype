# Teletype Eval Report

- **Git SHA:** 00f8a76
- **Timestamp:** 1792361094
- **Model:** sherpa-parakeet-v2-int8
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0993 |
| Macro WER | 0.1346 |
| Mean polish score | 43.1 |
| ASR p50 / p95 (ms) | 814 / 3492 |
| Total p50 / p95 (ms) | 814 / 3492 |
| Model load (ms) | 571 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| cv | 200 | 7349 | 730 | 0.0993 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| cv-0175 | 1.2000 | 4 | 0 | 2 | what sports is your favorite? | But the sports easy of a barrel. |
| cv-0139 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0145 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0150 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0190 | 0.8000 | 3 | 0 | 1 | i'll be running so fast. | I will run it so fast. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 132 |
| no_hallucination | 111 |
| no_fillers | 75 |
| terminal_punctuation | 14 |
| bullets_when_listing | 4 |
| sentence_cased | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 197 | 143 | 7262 | 1246 | 814 | 3492 | 6325 |
| total | 197 | 143 | 7262 | 1246 | 814 | 3492 | 6325 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0699 |
| Verbatim WER (macro) | 0.0982 |
| Entity accuracy | 0.8235 |

