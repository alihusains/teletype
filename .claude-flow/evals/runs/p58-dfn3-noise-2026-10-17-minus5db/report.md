# Teletype Eval Report

- **Git SHA:** 98f116a
- **Timestamp:** 1792209402
- **Model:** parakeet-ultra
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.1387 |
| Macro WER | 0.1597 |
| Mean polish score | 46.0 |
| ASR p50 / p95 (ms) | 145 / 645 |
| Total p50 / p95 (ms) | 145 / 645 |
| Model load (ms) | 6282 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 228 | 0.1387 |
| snr--5db | 100 | 1644 | 228 | 0.1387 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise--5db-0066 | 1.0000 | 4 | 9 | 0 | he has tried that road and knows that it is the wrong road | Yes, pride for those. |
| noise--5db-0025 | 0.8571 | 4 | 2 | 0 | the vil stuck fast in his throat | You feel stuck, stuff, start. |
| noise--5db-0033 | 0.8571 | 2 | 4 | 0 | but you are a jolly fellow too | You shot fetch. |
| noise--5db-0042 | 0.8000 | 5 | 3 | 0 | sit down beside me and i'll tell you the story | Sit down the side. I'm not pleased. |
| noise--5db-0087 | 0.8000 | 4 | 0 | 0 | not you only a sham | Mm-hmm. Give me a shame. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 60 |
| no_hallucination | 59 |
| sentence_cased | 7 |
| terminal_punctuation | 4 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 75 | 739 | 230 | 145 | 645 | 707 |
| total | 100 | 75 | 739 | 230 | 145 | 645 | 707 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1369 |
| Verbatim WER (macro) | 0.1575 |

