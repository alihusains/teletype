# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794279211
- **Model:** nemotron-3.5-coreml-multilingual
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 6 |
| Micro WER | 0.2358 |
| Macro WER | 0.2360 |
| Mean polish score | 4.0 |
| ASR p50 / p95 (ms) | 19975 / 21467 |
| Total p50 / p95 (ms) | 19975 / 21467 |
| Model load (ms) | 133 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| 4468919_trimmed | 1 | 2125 | 541 | 0.2546 |
| 4471586 | 1 | 2961 | 796 | 0.2688 |
| 4474955 | 1 | 1738 | 422 | 0.2428 |
| 4475604 | 1 | 2797 | 232 | 0.0829 |
| 4482968 | 1 | 3412 | 870 | 0.2550 |
| 4483046 | 1 | 2781 | 868 | 0.3121 |
| earnings22-vocab | 6 | 15814 | 3729 | 0.2358 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| earnings22-vocab-0006 | 0.3121 | 457 | 176 | 235 | now it's on times. they would like to starts the performance… | Now it's some time they would like to start the performance … |
| earnings22-vocab-0002 | 0.2688 | 331 | 386 | 79 | please stand by, we're about to begin. good day and welcome … | Please stand by. We're about to begin. Good day and welcome … |
| earnings22-vocab-0005 | 0.2550 | 454 | 248 | 168 | thank you, darcy, and welcome everyone to our december quart… | Thank you Darcy and welcome everyone to our December Quarter… |
| earnings22-vocab-0001 | 0.2546 | 300 | 72 | 169 | good morning and welcome to the loma negra third quarter 202… | Good morning and welcome to the Loma Negra third quarter twe… |
| earnings22-vocab-0003 | 0.2428 | 192 | 62 | 168 | hello ladies and gentlemen, thank you for standing by for qu… | Hello ladies and gentlemen, thank you for standing by Pudin'… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| bullets_when_listing | 6 |
| no_content_loss | 6 |
| no_fillers | 6 |
| no_hallucination | 6 |
| terminal_punctuation | 6 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 6 | 14659 | 21467 | 18697 | 19975 | 21467 | 21467 |
| total | 6 | 14659 | 21467 | 18697 | 19975 | 21467 | 21467 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1317 |
| Verbatim WER (macro) | 0.1315 |
| Vocab recall | 0.7392 |
| Vocab false-alarm rate | 0.0000 |
| Entity accuracy | 0.6839 |

