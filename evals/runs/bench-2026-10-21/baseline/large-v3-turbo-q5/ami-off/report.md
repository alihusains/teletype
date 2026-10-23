# Teletype Eval Report

- **Git SHA:** cee77e6
- **Timestamp:** 1792633149
- **Model:** large-v3-turbo-q5
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
| Macro WER | 0.2400 |
| Mean polish score | 64.3 |
| ASR p50 / p95 (ms) | 646 / 679 |
| Total p50 / p95 (ms) | 646 / 679 |
| Model load (ms) | 6331 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| ami | 100 | 761 | 145 | 0.1905 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| ami-0058 | 3.0000 | 1 | 0 | 2 | only | Oh my God. |
| ami-0029 | 2.0000 | 0 | 0 | 4 | and maybe | - And maybe-- - Always bearing in mind |
| ami-0009 | 1.0000 | 1 | 0 | 0 | mm-hmm | - Yeah. |
| ami-0015 | 1.0000 | 1 | 0 | 0 | yeah | Mm-hmm. |
| ami-0019 | 1.0000 | 1 | 0 | 0 | oops | Absolutely. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 47 |
| no_hallucination | 28 |
| sentence_cased | 15 |
| terminal_punctuation | 12 |
| no_fillers | 7 |
| no_markdown_fences | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 632 | 698 | 651 | 646 | 679 | 687 |
| total | 100 | 632 | 698 | 651 | 646 | 679 | 687 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1298 |
| Verbatim WER (macro) | 0.1617 |
| Entity accuracy | 0.8333 |

