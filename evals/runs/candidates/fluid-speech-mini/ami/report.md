# Teletype Eval Report

- **Git SHA:** 19b2767
- **Timestamp:** 1793033941
- **Model:** fluid-speech-mini
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.1340 |
| Macro WER | 0.1981 |
| Mean polish score | 68.8 |
| ASR p50 / p95 (ms) | 98 / 178 |
| Total p50 / p95 (ms) | 98 / 178 |
| Model load (ms) | 330 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| ami | 100 | 761 | 102 | 0.1340 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| ami-0006 | 1.0000 | 1 | 0 | 0 | mm | Mm-hmm. |
| ami-0009 | 1.0000 | 0 | 1 | 0 | mm-hmm |  |
| ami-0015 | 1.0000 | 0 | 1 | 0 | yeah |  |
| ami-0018 | 1.0000 | 0 | 1 | 0 | so |  |
| ami-0019 | 1.0000 | 0 | 1 | 0 | oops |  |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 41 |
| no_hallucination | 23 |
| terminal_punctuation | 23 |
| no_fillers | 16 |
| sentence_cased | 12 |
| no_markdown_fences | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 91 | 56 | 303 | 109 | 98 | 178 | 303 |
| total | 91 | 56 | 303 | 109 | 98 | 178 | 303 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0927 |
| Verbatim WER (macro) | 0.1195 |
| Entity accuracy | 0.8333 |

