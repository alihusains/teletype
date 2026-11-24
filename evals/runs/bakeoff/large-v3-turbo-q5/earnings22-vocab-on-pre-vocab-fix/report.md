# Teletype Eval Report

- **Git SHA:** d4b433b
- **Timestamp:** 1792228437
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
| Micro WER | 0.6774 |
| Macro WER | 0.6929 |
| Mean polish score | 6.3 |
| ASR p50 / p95 (ms) | 41863 / 43340 |
| Total p50 / p95 (ms) | 41863 / 43340 |
| Model load (ms) | 2158 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| 4468919_trimmed | 1 | 2125 | 1855 | 0.8729 |
| 4471586 | 1 | 2961 | 1936 | 0.6538 |
| 4474955 | 1 | 1738 | 1450 | 0.8343 |
| 4475604 | 1 | 2797 | 1097 | 0.3922 |
| 4482968 | 1 | 3412 | 2540 | 0.7444 |
| 4483046 | 1 | 2781 | 1835 | 0.6598 |
| earnings22-vocab | 6 | 15814 | 10713 | 0.6774 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| earnings22-vocab-0001 | 0.8729 | 54 | 1793 | 8 | good morning and welcome to the loma negra third quarter 202… | Good morning and welcome to the Loma Negra Third Quarter 202… |
| earnings22-vocab-0003 | 0.8343 | 37 | 1411 | 2 | hello ladies and gentlemen, thank you for standing by for qu… | Hello, Ladies and Gentlemen, Thank you for standing by for Q… |
| earnings22-vocab-0005 | 0.7444 | 262 | 2215 | 63 | thank you, darcy, and welcome everyone to our december quart… | Thank you Darcy and welcome everyone to our December quarter… |
| earnings22-vocab-0006 | 0.6598 | 142 | 1641 | 52 | now it's on times. they would like to starts the performance… | Now it's on time. I would like to start the performance brie… |
| earnings22-vocab-0002 | 0.6538 | 167 | 1723 | 46 | please stand by, we're about to begin. good day and welcome … | Please stand by, we're about to begin. Good day and welcome … |

## Polish Failed Checks

| Check | Failures |
|---|---|
| bullets_when_listing | 6 |
| no_content_loss | 6 |
| no_hallucination | 6 |
| no_fillers | 5 |
| terminal_punctuation | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 6 | 27719 | 43340 | 37630 | 41863 | 43340 | 43340 |
| total | 6 | 27719 | 43340 | 37630 | 41863 | 43340 | 43340 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.6598 |
| Verbatim WER (macro) | 0.6771 |
| Vocab recall | 0.5224 |
| Vocab false-alarm rate | 0.0000 |
| Entity accuracy | 0.2477 |

