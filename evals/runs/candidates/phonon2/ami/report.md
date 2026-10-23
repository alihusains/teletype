# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792775364
- **Model:** phonon2
- **Mode:** replay

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.1196 |
| Macro WER | 0.2015 |
| Mean polish score | 68.3 |
| ASR p50 / p95 (ms) | 9 / 24 |
| Total p50 / p95 (ms) | 9 / 24 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| ami | 100 | 761 | 91 | 0.1196 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| ami-0029 | 2.0000 | 0 | 0 | 4 | and maybe | And maybe bearing in mind that |
| ami-0044 | 2.0000 | 1 | 0 | 1 | mm-hmm | And so |
| ami-0058 | 2.0000 | 1 | 0 | 1 | only | Oh my |
| ami-0009 | 1.0000 | 1 | 0 | 0 | mm-hmm | Yeah. |
| ami-0015 | 1.0000 | 0 | 1 | 0 | yeah |  |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 38 |
| no_hallucination | 28 |
| terminal_punctuation | 25 |
| no_fillers | 18 |
| sentence_cased | 8 |
| no_markdown_fences | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 6 | 88346 | 1469 | 9 | 24 | 57139 |
| total | 100 | 6 | 88346 | 1469 | 9 | 24 | 57139 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1086 |
| Verbatim WER (macro) | 0.1367 |
| Entity accuracy | 0.8333 |

