# Teletype Eval Report

- **Git SHA:** d4b433b
- **Timestamp:** 1792224251
- **Model:** parakeet-tdt-v3
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 6 |
| Micro WER | 1.0000 |
| Macro WER | 1.0000 |
| Mean polish score | 45.0 |
| ASR p50 / p95 (ms) | 14550 / 18682 |
| Total p50 / p95 (ms) | 14550 / 18682 |
| Model load (ms) | 234 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| 4468919_trimmed | 1 | 2125 | 2125 | 1.0000 |
| 4471586 | 1 | 2961 | 2961 | 1.0000 |
| 4474955 | 1 | 1738 | 1738 | 1.0000 |
| 4475604 | 1 | 2797 | 2797 | 1.0000 |
| 4482968 | 1 | 3412 | 3412 | 1.0000 |
| 4483046 | 1 | 2781 | 2781 | 1.0000 |
| earnings22-vocab | 6 | 15814 | 15814 | 1.0000 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| earnings22-vocab-0001 | 1.0000 | 0 | 2125 | 0 | good morning and welcome to the loma negra third quarter 202… |  |
| earnings22-vocab-0002 | 1.0000 | 0 | 2961 | 0 | please stand by, we're about to begin. good day and welcome … |  |
| earnings22-vocab-0003 | 1.0000 | 0 | 1738 | 0 | hello ladies and gentlemen, thank you for standing by for qu… |  |
| earnings22-vocab-0004 | 1.0000 | 0 | 2797 | 0 | on today's call, we will be referring to the form 20-f and p… |  |
| earnings22-vocab-0005 | 1.0000 | 0 | 3412 | 0 | thank you, darcy, and welcome everyone to our december quart… |  |

## Polish Failed Checks

| Check | Failures |
|---|---|
| bullets_when_listing | 6 |
| no_content_loss | 6 |
| sentence_cased | 6 |
| terminal_punctuation | 6 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 6 | 10340 | 18682 | 14480 | 14550 | 18682 | 18682 |
| total | 6 | 10340 | 18682 | 14480 | 14550 | 18682 | 18682 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 1.0000 |
| Verbatim WER (macro) | 1.0000 |
| Vocab recall | 0.0000 |
| Vocab false-alarm rate | 0.0000 |
| Entity accuracy | 0.0000 |

