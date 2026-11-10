# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794279447
- **Model:** nemotron-3.5-coreml-multilingual
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.2641 |
| Macro WER | 0.5451 |
| Mean polish score | 48.3 |
| ASR p50 / p95 (ms) | 58 / 167 |
| Total p50 / p95 (ms) | 58 / 167 |
| Model load (ms) | 136 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| ami | 100 | 761 | 201 | 0.2641 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| ami-0002 | 1.0000 | 0 | 1 | 0 | okay |  |
| ami-0003 | 1.0000 | 0 | 1 | 0 | okay |  |
| ami-0004 | 1.0000 | 0 | 1 | 0 | no |  |
| ami-0006 | 1.0000 | 0 | 1 | 0 | mm |  |
| ami-0007 | 1.0000 | 0 | 1 | 0 | um |  |

## Polish Failed Checks

| Check | Failures |
|---|---|
| terminal_punctuation | 100 |
| no_content_loss | 80 |
| sentence_cased | 44 |
| no_hallucination | 20 |
| no_fillers | 10 |
| no_markdown_fences | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 12 | 341 | 75 | 58 | 167 | 293 |
| total | 100 | 12 | 341 | 75 | 58 | 167 | 293 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.2238 |
| Verbatim WER (macro) | 0.4207 |
| Entity accuracy | 0.5000 |

