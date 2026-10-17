# Teletype Eval Report

- **Git SHA:** d4b433b
- **Timestamp:** 1792224339
- **Model:** parakeet-tdt-v3
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.1222 |
| Macro WER | 0.1834 |
| Mean polish score | 70.8 |
| ASR p50 / p95 (ms) | 238 / 378 |
| Total p50 / p95 (ms) | 238 / 378 |
| Model load (ms) | 238 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| ami | 100 | 761 | 93 | 0.1222 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| ami-0029 | 2.0000 | 1 | 0 | 3 | and maybe | And bearing in mind that |
| ami-0058 | 2.0000 | 1 | 0 | 1 | only | Oh my |
| ami-0009 | 1.0000 | 1 | 0 | 0 | mm-hmm | Mm. |
| ami-0018 | 1.0000 | 1 | 0 | 0 | so | Excellent. |
| ami-0019 | 1.0000 | 0 | 1 | 0 | oops |  |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 33 |
| no_hallucination | 29 |
| terminal_punctuation | 18 |
| no_fillers | 16 |
| sentence_cased | 3 |
| no_markdown_fences | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 99 | 131 | 553 | 249 | 238 | 378 | 553 |
| total | 99 | 131 | 553 | 249 | 238 | 378 | 553 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1020 |
| Verbatim WER (macro) | 0.1329 |
| Entity accuracy | 0.8333 |

