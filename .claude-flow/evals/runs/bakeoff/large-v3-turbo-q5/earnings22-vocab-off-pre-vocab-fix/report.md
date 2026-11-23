# Teletype Eval Report

- **Git SHA:** d4b433b
- **Timestamp:** 1792229681
- **Model:** large-v3-turbo-q5
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 6 |
| Micro WER | 0.7122 |
| Macro WER | 0.7192 |
| Mean polish score | 8.0 |
| ASR p50 / p95 (ms) | 34044 / 45435 |
| Total p50 / p95 (ms) | 34044 / 45435 |
| Model load (ms) | 2223 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| 4468919_trimmed | 1 | 2125 | 1396 | 0.6569 |
| 4471586 | 1 | 2961 | 2505 | 0.8460 |
| 4474955 | 1 | 1738 | 1468 | 0.8446 |
| 4475604 | 1 | 2797 | 2385 | 0.8527 |
| 4482968 | 1 | 3412 | 2207 | 0.6468 |
| 4483046 | 1 | 2781 | 1301 | 0.4678 |
| earnings22-vocab | 6 | 15814 | 11262 | 0.7122 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| earnings22-vocab-0004 | 0.8527 | 59 | 2316 | 10 | on today's call, we will be referring to the form 20-f and p… | Thank you. Thank you. Thank you. Thank you. Thank you. Thank… |
| earnings22-vocab-0002 | 0.8460 | 85 | 2408 | 12 | please stand by, we're about to begin. good day and welcome … | Thank you. Thank you. Thank you. Thank you. Thank you. Thank… |
| earnings22-vocab-0003 | 0.8446 | 54 | 1413 | 1 | hello ladies and gentlemen, thank you for standing by for qu… | Hello, ladies and gentlemen. Thank you for standing by for Q… |
| earnings22-vocab-0001 | 0.6569 | 98 | 1281 | 17 | good morning and welcome to the loma negra third quarter 202… | Good morning and welcome to the Loma Negra 3rd Quarter 2021 … |
| earnings22-vocab-0005 | 0.6468 | 148 | 1984 | 75 | thank you, darcy, and welcome everyone to our december quart… | Thank you, Darcy, and welcome everyone to our December quart… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| bullets_when_listing | 6 |
| no_content_loss | 6 |
| no_hallucination | 6 |
| no_fillers | 3 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 6 | 25874 | 45435 | 35387 | 34044 | 45435 | 45435 |
| total | 6 | 25874 | 45435 | 35387 | 34044 | 45435 | 45435 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.6962 |
| Verbatim WER (macro) | 0.7038 |
| Vocab recall | 0.4980 |
| Vocab false-alarm rate | 0.0000 |
| Entity accuracy | 0.2884 |

