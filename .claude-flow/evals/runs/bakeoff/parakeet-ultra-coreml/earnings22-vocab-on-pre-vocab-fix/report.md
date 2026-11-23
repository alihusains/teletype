# Teletype Eval Report

- **Git SHA:** d4b433b
- **Timestamp:** 1792225701
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
| Micro WER | 0.1733 |
| Macro WER | 0.1704 |
| Mean polish score | 6.0 |
| ASR p50 / p95 (ms) | 21488 / 42558 |
| Total p50 / p95 (ms) | 21488 / 42558 |
| Model load (ms) | 89 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| 4468919_trimmed | 1 | 2125 | 405 | 0.1906 |
| 4471586 | 1 | 2961 | 590 | 0.1993 |
| 4474955 | 1 | 1738 | 246 | 0.1415 |
| 4475604 | 1 | 2797 | 304 | 0.1087 |
| 4482968 | 1 | 3412 | 714 | 0.2093 |
| 4483046 | 1 | 2781 | 482 | 0.1733 |
| earnings22-vocab | 6 | 15814 | 2741 | 0.1733 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| earnings22-vocab-0005 | 0.2093 | 500 | 160 | 54 | thank you, darcy, and welcome everyone to our december quart… | Thank you, Darcy, and welcome everyone to our December Quart… |
| earnings22-vocab-0002 | 0.1993 | 319 | 218 | 53 | please stand by, we're about to begin. good day and welcome … | Please And by, we're about to begin. Good Dan and welcome to… |
| earnings22-vocab-0001 | 0.1906 | 307 | 38 | 60 | good morning and welcome to the loma negra third quarter 202… | Good morning and welcome to Loma Negra third quarter 2021 Co… |
| earnings22-vocab-0006 | 0.1733 | 280 | 161 | 41 | now it's on times. they would like to starts the performance… | Now it's on time. They would like to start the performance b… |
| earnings22-vocab-0003 | 0.1415 | 184 | 32 | 30 | hello ladies and gentlemen, thank you for standing by for qu… | Hello, ladies and gentlemen, thank you for standing by for C… |

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
| asr | 6 | 14821 | 42558 | 24673 | 21488 | 42558 | 42558 |
| total | 6 | 14821 | 42558 | 24673 | 21488 | 42558 | 42558 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1442 |
| Verbatim WER (macro) | 0.1431 |
| Vocab recall | 0.9539 |
| Vocab false-alarm rate | 0.0000 |
| Entity accuracy | 0.7301 |

