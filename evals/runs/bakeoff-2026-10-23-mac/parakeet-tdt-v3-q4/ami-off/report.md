# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792752041
- **Model:** parakeet-tdt-v3-q4
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.1261 |
| Macro WER | 0.1786 |
| Mean polish score | 71.4 |
| ASR p50 / p95 (ms) | 75 / 209 |
| Total p50 / p95 (ms) | 75 / 209 |
| Model load (ms) | 150 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| ami | 100 | 761 | 96 | 0.1261 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| ami-0058 | 2.0000 | 1 | 0 | 1 | only | Oh my. |
| ami-0091 | 1.2500 | 1 | 0 | 4 | so it can rest | So it can be a little bit more. |
| ami-0018 | 1.0000 | 0 | 1 | 0 | so |  |
| ami-0019 | 1.0000 | 1 | 0 | 0 | oops | Yeah. |
| ami-0042 | 1.0000 | 1 | 0 | 0 | 'kay | Okay. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 34 |
| no_hallucination | 26 |
| no_fillers | 17 |
| terminal_punctuation | 16 |
| sentence_cased | 5 |
| no_markdown_fences | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 99 | 41 | 366 | 95 | 75 | 209 | 366 |
| total | 99 | 41 | 366 | 95 | 75 | 209 | 366 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0993 |
| Verbatim WER (macro) | 0.1353 |
| Entity accuracy | 0.8333 |

