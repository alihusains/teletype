# Teletype Eval Report

- **Git SHA:** 981900e
- **Timestamp:** 1792697692
- **Model:** parakeet-ultra-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 6 |
| Micro WER | 0.1439 |
| Macro WER | 0.1428 |
| Mean polish score | 6.0 |
| ASR p50 / p95 (ms) | 19712 / 25823 |
| Total p50 / p95 (ms) | 19712 / 25823 |
| Model load (ms) | 134 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| 4468919_trimmed | 1 | 2125 | 339 | 0.1595 |
| 4471586 | 1 | 2961 | 504 | 0.1702 |
| 4474955 | 1 | 1738 | 226 | 0.1300 |
| 4475604 | 1 | 2797 | 258 | 0.0922 |
| 4482968 | 1 | 3412 | 546 | 0.1600 |
| 4483046 | 1 | 2781 | 403 | 0.1449 |
| earnings22-vocab | 6 | 15814 | 2276 | 0.1439 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| earnings22-vocab-0002 | 0.1702 | 234 | 217 | 53 | please stand by, we're about to begin. good day and welcome … | Please stand by, we're about to begin. Good Dan and welcome … |
| earnings22-vocab-0005 | 0.1600 | 336 | 157 | 53 | thank you, darcy, and welcome everyone to our december quart… | Thank you, Darcy, and welcome everyone to our December quart… |
| earnings22-vocab-0001 | 0.1595 | 239 | 36 | 64 | good morning and welcome to the loma negra third quarter 202… | Good morning and welcome to Loma Negra third quarter 2021 Co… |
| earnings22-vocab-0006 | 0.1449 | 201 | 160 | 42 | now it's on times. they would like to starts the performance… | Now it's on time. They would like to start the performance b… |
| earnings22-vocab-0003 | 0.1300 | 165 | 31 | 30 | hello ladies and gentlemen, thank you for standing by for qu… | Hello, ladies and gentlemen, Frank you for standing by for Q… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| bullets_when_listing | 6 |
| no_content_loss | 6 |
| no_fillers | 6 |
| no_hallucination | 6 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 6 | 14595 | 25823 | 20330 | 19712 | 25823 | 25823 |
| total | 6 | 14595 | 25823 | 20330 | 19712 | 25823 | 25823 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1061 |
| Verbatim WER (macro) | 0.1065 |
| Vocab recall | 0.9526 |
| Vocab false-alarm rate | 0.0000 |
| Entity accuracy | 0.7542 |

