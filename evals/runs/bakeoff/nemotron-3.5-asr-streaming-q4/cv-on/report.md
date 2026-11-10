# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794277601
- **Model:** nemotron-3.5-asr-streaming-q4
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.1381 |
| Macro WER | 0.2261 |
| Mean polish score | 28.7 |
| ASR p50 / p95 (ms) | 170 / 706 |
| Total p50 / p95 (ms) | 170 / 706 |
| Model load (ms) | 70 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| cv | 200 | 7349 | 1015 | 0.1381 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| cv-0008 | 1.0000 | 0 | 12 | 0 | yeah, i'll go and see a doctor if i have an ailment. |  |
| cv-0012 | 1.0000 | 0 | 9 | 0 | i use my phone, my mobile phone very frequently |  |
| cv-0020 | 1.0000 | 0 | 11 | 0 | world war one, world war two. we learn it in school. |  |
| cv-0106 | 1.0000 | 1 | 2 | 0 | no, i don't. | Not. |
| cv-0113 | 1.0000 | 0 | 3 | 0 | yes, i do. |  |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 164 |
| no_hallucination | 138 |
| terminal_punctuation | 89 |
| no_fillers | 78 |
| sentence_cased | 14 |
| bullets_when_listing | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 189 | 37 | 1231 | 240 | 170 | 706 | 1050 |
| total | 189 | 37 | 1231 | 240 | 170 | 706 | 1050 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1062 |
| Verbatim WER (macro) | 0.1832 |
| Entity accuracy | 0.7647 |

