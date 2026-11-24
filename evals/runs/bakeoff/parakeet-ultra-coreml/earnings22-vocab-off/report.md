# Teletype Eval Report

- **Git SHA:** 981900e
- **Timestamp:** 1792697664
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
| Micro WER | 0.1090 |
| Macro WER | 0.1073 |
| Mean polish score | 6.0 |
| ASR p50 / p95 (ms) | 2592 / 3192 |
| Total p50 / p95 (ms) | 2592 / 3192 |
| Model load (ms) | 11506 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| 4468919_trimmed | 1 | 2125 | 228 | 0.1073 |
| 4471586 | 1 | 2961 | 420 | 0.1418 |
| 4474955 | 1 | 1738 | 164 | 0.0944 |
| 4475604 | 1 | 2797 | 140 | 0.0501 |
| 4482968 | 1 | 3412 | 405 | 0.1187 |
| 4483046 | 1 | 2781 | 366 | 0.1316 |
| earnings22-vocab | 6 | 15814 | 1723 | 0.1090 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| earnings22-vocab-0002 | 0.1418 | 158 | 210 | 52 | please stand by, we're about to begin. good day and welcome … | Please stand by, we're about to begin. Good day and welcome … |
| earnings22-vocab-0006 | 0.1316 | 164 | 160 | 42 | now it's on times. they would like to starts the performance… | Now it's on time. They would like to start the performance b… |
| earnings22-vocab-0005 | 0.1187 | 223 | 124 | 58 | thank you, darcy, and welcome everyone to our december quart… | Thank you, Darcy, and welcome everyone to our December quart… |
| earnings22-vocab-0001 | 0.1073 | 139 | 25 | 64 | good morning and welcome to the loma negra third quarter 202… | Good morning and welcome to the Loma Negra third quarter 202… |
| earnings22-vocab-0003 | 0.0944 | 107 | 26 | 31 | hello ladies and gentlemen, thank you for standing by for qu… | Hello, ladies and gentlemen, thank you for standing by for C… |

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
| asr | 6 | 2096 | 3192 | 2659 | 2592 | 3192 | 3192 |
| total | 6 | 2096 | 3192 | 2659 | 2592 | 3192 | 3192 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0688 |
| Verbatim WER (macro) | 0.0689 |
| Vocab recall | 0.8384 |
| Vocab false-alarm rate | 0.0000 |
| Entity accuracy | 0.8447 |

