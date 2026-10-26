# Teletype Eval Report

- **Git SHA:** 19b2767
- **Timestamp:** 1793036547
- **Model:** fluid-speech-mini
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0613 |
| Macro WER | 0.0684 |
| Mean polish score | 70.4 |
| ASR p50 / p95 (ms) | 165 / 424 |
| Total p50 / p95 (ms) | 165 / 424 |
| Model load (ms) | 178 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| competing-speaker | 200 | 4126 | 253 | 0.0613 |
| librispeech | 200 | 4126 | 253 | 0.0613 |
| ssr-5db | 200 | 4126 | 253 | 0.0613 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| competing-5db-0048 | 1.0000 | 10 | 0 | 1 | oh no jasper i must go by my very own self | You use our words, Mrs Huntle, but I very can pardon you. |
| competing-5db-0161 | 1.0000 | 9 | 4 | 0 | i say you do know what this means and you must tell us | When I wake up just to day break, he |
| competing-5db-0110 | 0.9545 | 19 | 0 | 2 | i made her for only twenty oars because i thought few men wo… | I made her following her way the further side into the river… |
| competing-5db-0195 | 0.7805 | 22 | 10 | 0 | guided by you how we might stroll towards death our only mus… | In the end, how we tarried some time in this solace and sati… |
| competing-5db-0100 | 0.7500 | 0 | 0 | 3 | i will practice it | I will practice it, she put her |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 64 |
| no_hallucination | 64 |
| terminal_punctuation | 41 |
| sentence_cased | 15 |
| no_fillers | 7 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 88 | 576 | 202 | 165 | 424 | 503 |
| total | 200 | 88 | 576 | 202 | 165 | 424 | 503 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0611 |
| Verbatim WER (macro) | 0.0679 |
| Entity accuracy | 0.7000 |

