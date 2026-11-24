# Teletype Eval Report

- **Git SHA:** 98f116a
- **Timestamp:** 1792209436
- **Model:** parakeet-ultra
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 800 |
| Micro WER | 0.0924 |
| Macro WER | 0.1136 |
| Mean polish score | 55.8 |
| ASR p50 / p95 (ms) | 140 / 310 |
| Total p50 / p95 (ms) | 140 / 310 |
| Model load (ms) | 197 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 800 | 14020 | 1296 | 0.0924 |
| snr--5db | 200 | 3505 | 565 | 0.1612 |
| snr-0db | 200 | 3505 | 344 | 0.0981 |
| snr-10db | 200 | 3505 | 181 | 0.0516 |
| snr-5db | 200 | 3505 | 206 | 0.0588 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| libri-noise--5db-0098 | 1.2500 | 4 | 0 | 1 | say you know sumthin | So we don't stop it. |
| libri-noise--5db-0006 | 1.0000 | 2 | 5 | 0 | personally as you call it and all | Даст Дол. |
| libri-noise--5db-0116 | 1.0000 | 4 | 5 | 0 | i have done well to record them so minutely | Эд Данвелт рекордник коммунист. |
| libri-noise--5db-0121 | 1.0000 | 6 | 0 | 1 | you never tried me milly said he | You know what should I mean being said? |
| libri-noise--5db-0139 | 1.0000 | 0 | 5 | 0 | i can scarcely credit it |  |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 393 |
| no_hallucination | 384 |
| terminal_punctuation | 63 |
| sentence_cased | 46 |
| no_fillers | 19 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 798 | 73 | 864 | 168 | 140 | 310 | 645 |
| total | 798 | 73 | 864 | 168 | 140 | 310 | 645 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0894 |
| Verbatim WER (macro) | 0.1106 |
| Entity accuracy | 0.6667 |

