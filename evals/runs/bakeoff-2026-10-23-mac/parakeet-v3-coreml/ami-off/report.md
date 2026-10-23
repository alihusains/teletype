# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792751902
- **Model:** parakeet-v3-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.1432 |
| Macro WER | 0.2259 |
| Mean polish score | 69.7 |
| ASR p50 / p95 (ms) | 35 / 53 |
| Total p50 / p95 (ms) | 35 / 53 |
| Model load (ms) | 94 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| ami | 100 | 761 | 109 | 0.1432 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| ami-0029 | 2.5000 | 1 | 0 | 4 | and maybe | And always bearing in mind that |
| ami-0009 | 1.0000 | 0 | 1 | 0 | mm-hmm |  |
| ami-0011 | 1.0000 | 1 | 0 | 0 | mm-hmm | Mm. |
| ami-0015 | 1.0000 | 0 | 1 | 0 | yeah |  |
| ami-0018 | 1.0000 | 0 | 1 | 0 | so |  |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 37 |
| no_hallucination | 26 |
| terminal_punctuation | 25 |
| no_fillers | 16 |
| sentence_cased | 9 |
| no_markdown_fences | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 96 | 29 | 64 | 38 | 35 | 53 | 64 |
| total | 96 | 29 | 64 | 38 | 35 | 53 | 64 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1179 |
| Verbatim WER (macro) | 0.1621 |
| Entity accuracy | 0.8333 |

