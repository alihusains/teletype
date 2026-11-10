# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794279532
- **Model:** nemotron-3.5-coreml-multilingual
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.1314 |
| Macro WER | 0.2080 |
| Mean polish score | 27.3 |
| ASR p50 / p95 (ms) | 274 / 1128 |
| Total p50 / p95 (ms) | 274 / 1128 |
| Model load (ms) | 138 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| cv | 200 | 7349 | 966 | 0.1314 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| cv-0114 | 1.0000 | 1 | 4 | 0 | [noise] i have a [unclear|box/books?]. | アイハブ |
| cv-0118 | 1.0000 | 3 | 1 | 0 | my earliest memories was... | Maja resta memoris |
| cv-0139 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0145 | 1.0000 | 0 | 1 | 0 | [silence] |  |
| cv-0147 | 1.0000 | 0 | 8 | 0 | i rare- i rarely go to the park. |  |

## Polish Failed Checks

| Check | Failures |
|---|---|
| terminal_punctuation | 198 |
| no_content_loss | 161 |
| no_hallucination | 144 |
| no_fillers | 78 |
| sentence_cased | 10 |
| bullets_when_listing | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 43 | 1684 | 368 | 274 | 1128 | 1495 |
| total | 200 | 43 | 1684 | 368 | 274 | 1128 | 1495 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1036 |
| Verbatim WER (macro) | 0.1718 |
| Entity accuracy | 0.8235 |

