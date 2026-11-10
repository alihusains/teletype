# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794277448
- **Model:** nemotron-3.5-asr-streaming-q4
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 6 |
| Micro WER | 0.2404 |
| Macro WER | 0.2404 |
| Mean polish score | 4.3 |
| ASR p50 / p95 (ms) | 16509 / 18840 |
| Total p50 / p95 (ms) | 16509 / 18840 |
| Model load (ms) | 77 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| 4468919_trimmed | 1 | 2125 | 553 | 0.2602 |
| 4471586 | 1 | 2961 | 818 | 0.2763 |
| 4474955 | 1 | 1738 | 423 | 0.2434 |
| 4475604 | 1 | 2797 | 226 | 0.0808 |
| 4482968 | 1 | 3412 | 886 | 0.2597 |
| 4483046 | 1 | 2781 | 896 | 0.3222 |
| earnings22-vocab | 6 | 15814 | 3802 | 0.2404 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| earnings22-vocab-0006 | 0.3222 | 468 | 189 | 239 | now it's on times. they would like to starts the performance… | Now it's some time they will like to the start the performan… |
| earnings22-vocab-0002 | 0.2763 | 322 | 414 | 82 | please stand by, we're about to begin. good day and welcome … | Please stand by, we're about to begin. Good day and welcome … |
| earnings22-vocab-0001 | 0.2602 | 293 | 94 | 166 | good morning and welcome to the loma negra third quarter 202… | Good morning and welcome to the Lomanegra Third Quarter twen… |
| earnings22-vocab-0005 | 0.2597 | 449 | 274 | 163 | thank you, darcy, and welcome everyone to our december quart… | Thank you Darcy and welcome everyone to our December Quarter… |
| earnings22-vocab-0003 | 0.2434 | 200 | 61 | 162 | hello ladies and gentlemen, thank you for standing by for qu… | Hello ladies and gentlemen, thank you for standing by Procur… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| bullets_when_listing | 6 |
| no_content_loss | 6 |
| no_fillers | 6 |
| no_hallucination | 6 |
| terminal_punctuation | 5 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 6 | 12836 | 18840 | 16033 | 16509 | 18840 | 18840 |
| total | 6 | 12836 | 18840 | 16033 | 16509 | 18840 | 18840 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1357 |
| Verbatim WER (macro) | 0.1353 |
| Vocab recall | 0.7565 |
| Vocab false-alarm rate | 0.0000 |
| Entity accuracy | 0.6784 |

