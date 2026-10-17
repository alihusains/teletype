# Teletype Eval Report

- **Git SHA:** 1b15b10
- **Timestamp:** 1792269517
- **Model:** sherpa-parakeet-v3-int8
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.1393 |
| Macro WER | 0.2123 |
| Mean polish score | 67.5 |
| ASR p50 / p95 (ms) | 108 / 475 |
| Total p50 / p95 (ms) | 108 / 475 |
| Model load (ms) | 848 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| ami | 100 | 761 | 106 | 0.1393 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| ami-0029 | 2.0000 | 0 | 0 | 4 | and maybe | And maybe bearing in mind that |
| ami-0009 | 1.0000 | 1 | 0 | 0 | mm-hmm | Mm. |
| ami-0013 | 1.0000 | 1 | 0 | 0 | feedback | Feck. |
| ami-0015 | 1.0000 | 0 | 1 | 0 | yeah |  |
| ami-0018 | 1.0000 | 0 | 1 | 0 | so |  |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 41 |
| no_hallucination | 27 |
| terminal_punctuation | 19 |
| no_fillers | 16 |
| sentence_cased | 7 |
| no_markdown_fences | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 96 | 24 | 915 | 180 | 108 | 475 | 915 |
| total | 96 | 24 | 915 | 180 | 108 | 475 | 915 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1166 |
| Verbatim WER (macro) | 0.1616 |
| Entity accuracy | 0.8333 |

