# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794277550
- **Model:** nemotron-3.5-asr-streaming-q4
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.1905 |
| Macro WER | 0.3001 |
| Mean polish score | 59.2 |
| ASR p50 / p95 (ms) | 39 / 98 |
| Total p50 / p95 (ms) | 39 / 98 |
| Model load (ms) | 65 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| ami | 100 | 761 | 145 | 0.1905 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| ami-0004 | 1.0000 | 0 | 1 | 0 | no |  |
| ami-0006 | 1.0000 | 0 | 1 | 0 | mm |  |
| ami-0009 | 1.0000 | 1 | 0 | 0 | mm-hmm | Äh. |
| ami-0011 | 1.0000 | 0 | 1 | 0 | mm-hmm |  |
| ami-0015 | 1.0000 | 0 | 1 | 0 | yeah |  |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 57 |
| terminal_punctuation | 39 |
| no_hallucination | 26 |
| sentence_cased | 21 |
| no_fillers | 13 |
| no_markdown_fences | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 83 | 14 | 199 | 49 | 39 | 98 | 199 |
| total | 83 | 14 | 199 | 49 | 39 | 98 | 199 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1417 |
| Verbatim WER (macro) | 0.2095 |
| Entity accuracy | 0.8333 |

