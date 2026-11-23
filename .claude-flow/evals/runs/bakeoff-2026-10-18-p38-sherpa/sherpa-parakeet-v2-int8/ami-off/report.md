# Teletype Eval Report

- **Git SHA:** fa62b6d
- **Timestamp:** 1792270952
- **Model:** sherpa-parakeet-v2-int8
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
| Macro WER | 0.1789 |
| Mean polish score | 66.5 |
| ASR p50 / p95 (ms) | 103 / 440 |
| Total p50 / p95 (ms) | 103 / 440 |
| Model load (ms) | 556 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| ami | 100 | 761 | 102 | 0.1340 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| ami-0029 | 2.5000 | 1 | 0 | 4 | and maybe | And also bearing in mind that |
| ami-0015 | 1.0000 | 1 | 0 | 0 | yeah | The |
| ami-0018 | 1.0000 | 1 | 0 | 0 | so | Yeah. |
| ami-0019 | 1.0000 | 1 | 0 | 0 | oops | Mm-hmm. |
| ami-0058 | 1.0000 | 1 | 0 | 0 | only | Mm-hmm. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 38 |
| no_hallucination | 33 |
| terminal_punctuation | 17 |
| no_fillers | 15 |
| sentence_cased | 10 |
| no_markdown_fences | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 23 | 908 | 172 | 103 | 440 | 725 |
| total | 100 | 23 | 908 | 172 | 103 | 440 | 725 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1139 |
| Verbatim WER (macro) | 0.1481 |
| Entity accuracy | 0.8333 |

