# Teletype Eval Report

- **Git SHA:** fd63811
- **Timestamp:** 1793203293
- **Model:** lfm3
- **Mode:** replay

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.1840 |
| Macro WER | 0.2541 |
| Mean polish score | 56.2 |
| ASR p50 / p95 (ms) | 167 / 454 |
| Total p50 / p95 (ms) | 167 / 454 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| ami | 100 | 761 | 140 | 0.1840 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| ami-0058 | 3.0000 | 1 | 0 | 2 | only | Oh my god. |
| ami-0007 | 1.0000 | 1 | 0 | 0 | um | Uhm. |
| ami-0009 | 1.0000 | 1 | 0 | 0 | mm-hmm | mm |
| ami-0011 | 1.0000 | 1 | 0 | 0 | mm-hmm | mhm |
| ami-0040 | 1.0000 | 1 | 0 | 0 | mm-hmm | mhm |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 52 |
| no_hallucination | 43 |
| sentence_cased | 9 |
| terminal_punctuation | 6 |
| no_fillers | 5 |
| no_markdown_fences | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 79 | 634 | 210 | 167 | 454 | 581 |
| total | 100 | 79 | 634 | 210 | 167 | 454 | 581 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1126 |
| Verbatim WER (macro) | 0.1236 |
| Entity accuracy | 0.8333 |

