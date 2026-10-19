# Teletype Eval Report

- **Git SHA:** 00f8a76
- **Timestamp:** 1792362952
- **Model:** sherpa-parakeet-v3-int8
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.1211 |
| Macro WER | 0.1748 |
| Mean polish score | 40.1 |
| ASR p50 / p95 (ms) | 1262 / 5648 |
| Total p50 / p95 (ms) | 1262 / 5648 |
| Model load (ms) | 769 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| cv | 200 | 7349 | 890 | 0.1211 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| cv-0004 | 1.0000 | 0 | 34 | 0 | a strict budget for a family--i don't have [unclear] strict … |  |
| cv-0005 | 1.0000 | 0 | 14 | 0 | tunku abdul rahman the first prime minister of malaysia. yea… |  |
| cv-0014 | 1.0000 | 0 | 8 | 0 | because hope for the future, peace and harmony. |  |
| cv-0017 | 1.0000 | 0 | 16 | 0 | chewing is a healthy sport. chewing is good for those who ha… |  |
| cv-0018 | 1.0000 | 0 | 14 | 0 | [noise] [unclear] it was a long long time ago but it was [un… |  |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 136 |
| no_hallucination | 119 |
| no_fillers | 90 |
| terminal_punctuation | 17 |
| sentence_cased | 10 |
| bullets_when_listing | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 190 | 232 | 11154 | 1960 | 1262 | 5648 | 9645 |
| total | 190 | 232 | 11154 | 1960 | 1262 | 5648 | 9645 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1015 |
| Verbatim WER (macro) | 0.1432 |
| Entity accuracy | 0.7059 |

