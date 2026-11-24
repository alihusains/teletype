# Teletype Eval Report

- **Git SHA:** 00f8a76
- **Timestamp:** 1792362084
- **Model:** large-v3-turbo-q5
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.1312 |
| Macro WER | 0.1551 |
| Mean polish score | 41.5 |
| ASR p50 / p95 (ms) | 682 / 1569 |
| Total p50 / p95 (ms) | 682 / 1569 |
| Model load (ms) | 192 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| cv | 200 | 7349 | 964 | 0.1312 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| cv-0005 | 1.0000 | 4 | 10 | 0 | tunku abdul rahman the first prime minister of malaysia. yea… | Thank you very much. |
| cv-0139 | 1.0000 | 0 | 1 | 0 | [silence] | . |
| cv-0145 | 1.0000 | 1 | 0 | 0 | [silence] | you |
| cv-0147 | 1.0000 | 2 | 6 | 0 | i rare- i rarely go to the park. | Ivelugótu, Þabag. |
| cv-0150 | 1.0000 | 1 | 0 | 0 | [silence] | you |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 139 |
| no_hallucination | 111 |
| no_fillers | 64 |
| terminal_punctuation | 15 |
| sentence_cased | 10 |
| bullets_when_listing | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 641 | 2921 | 888 | 682 | 1569 | 2318 |
| total | 200 | 641 | 2921 | 888 | 682 | 1569 | 2318 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0967 |
| Verbatim WER (macro) | 0.1186 |
| Entity accuracy | 0.8235 |

