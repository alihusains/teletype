# Teletype Eval Report

- **Git SHA:** 981900e
- **Timestamp:** 1792699372
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
| Micro WER | 0.7065 |
| Macro WER | 0.7287 |
| Mean polish score | 5.3 |
| ASR p50 / p95 (ms) | 34582 / 44415 |
| Total p50 / p95 (ms) | 34582 / 44415 |
| Model load (ms) | 191 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| 4468919_trimmed | 1 | 2125 | 1855 | 0.8729 |
| 4471586 | 1 | 2961 | 2081 | 0.7028 |
| 4474955 | 1 | 1738 | 1467 | 0.8441 |
| 4475604 | 1 | 2797 | 2016 | 0.7208 |
| 4482968 | 1 | 3412 | 1781 | 0.5220 |
| 4483046 | 1 | 2781 | 1973 | 0.7095 |
| earnings22-vocab | 6 | 15814 | 11173 | 0.7065 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| earnings22-vocab-0001 | 0.8729 | 54 | 1793 | 8 | good morning and welcome to the loma negra third quarter 202… | Good morning and welcome to the Loma Negra Third Quarter 202… |
| earnings22-vocab-0003 | 0.8441 | 44 | 1421 | 2 | hello ladies and gentlemen, thank you for standing by for qu… | Hello ladies and gentlemen, thank you for standing by for Qu… |
| earnings22-vocab-0004 | 0.7208 | 114 | 1884 | 18 | on today's call, we will be referring to the form 20-f and p… | On today's call, we will be referring to the Form 20F and pr… |
| earnings22-vocab-0006 | 0.7095 | 120 | 1836 | 17 | now it's on times. they would like to starts the performance… | Now it's on time. I would like to start the performance brie… |
| earnings22-vocab-0002 | 0.7028 | 143 | 1860 | 78 | please stand by, we're about to begin. good day and welcome … | Please stand by. We're about to begin. Good day and welcome … |

## Polish Failed Checks

| Check | Failures |
|---|---|
| bullets_when_listing | 6 |
| no_content_loss | 6 |
| no_fillers | 6 |
| no_hallucination | 6 |
| terminal_punctuation | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 6 | 25094 | 44415 | 33914 | 34582 | 44415 | 44415 |
| total | 6 | 25094 | 44415 | 33914 | 34582 | 44415 | 44415 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.6894 |
| Verbatim WER (macro) | 0.7116 |
| Vocab recall | 0.4095 |
| Vocab false-alarm rate | 0.0000 |
| Entity accuracy | 0.1978 |

