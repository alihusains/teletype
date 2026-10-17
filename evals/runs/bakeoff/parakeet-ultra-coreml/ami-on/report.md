# Teletype Eval Report

- **Git SHA:** d4b433b
- **Timestamp:** 1792225881
- **Model:** parakeet-ultra-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.1353 |
| Macro WER | 0.1933 |
| Mean polish score | 68.7 |
| ASR p50 / p95 (ms) | 38 / 56 |
| Total p50 / p95 (ms) | 38 / 56 |
| Model load (ms) | 93 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| ami | 100 | 761 | 103 | 0.1353 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| ami-0058 | 2.0000 | 1 | 0 | 1 | only | No way. |
| ami-0029 | 1.5000 | 0 | 0 | 3 | and maybe | And maybe in mind that |
| ami-0009 | 1.0000 | 0 | 1 | 0 | mm-hmm |  |
| ami-0015 | 1.0000 | 0 | 1 | 0 | yeah |  |
| ami-0018 | 1.0000 | 1 | 0 | 0 | so | Excellent. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 38 |
| no_hallucination | 27 |
| terminal_punctuation | 24 |
| no_fillers | 16 |
| sentence_cased | 12 |
| no_markdown_fences | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 96 | 31 | 62 | 40 | 38 | 56 | 62 |
| total | 96 | 31 | 62 | 40 | 38 | 56 | 62 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1046 |
| Verbatim WER (macro) | 0.1394 |
| Entity accuracy | 0.8333 |

