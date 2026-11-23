# Teletype Eval Report

- **Git SHA:** 19b2767
- **Timestamp:** 1793036701
- **Model:** parakeet-ultra-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0783 |
| Macro WER | 0.0876 |
| Mean polish score | 69.4 |
| ASR p50 / p95 (ms) | 54 / 159 |
| Total p50 / p95 (ms) | 54 / 159 |
| Model load (ms) | 116 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| competing-speaker | 200 | 4126 | 323 | 0.0783 |
| librispeech | 200 | 4126 | 323 | 0.0783 |
| ssr-5db | 200 | 4126 | 323 | 0.0783 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| competing-5db-0195 | 1.3171 | 40 | 0 | 14 | guided by you how we might stroll towards death our only mus… | in merry day and night, and I tarried some time in this sola… |
| competing-5db-0048 | 1.0000 | 10 | 0 | 1 | oh no jasper i must go by my very own self | You use hard words, Mrs. Huntington, but I very don't pardon… |
| competing-5db-0097 | 1.0000 | 21 | 4 | 0 | but philip did afford it and he wrote thanking his friends a… | And so whip me off this glass neatly, bring me hither some c… |
| competing-5db-0100 | 1.0000 | 0 | 0 | 4 | i will practice it | I will practice it. She put her hand. |
| competing-5db-0110 | 0.9091 | 18 | 2 | 0 | i made her for only twenty oars because i thought few men wo… | David went her way further south into the river, and said sh… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 69 |
| no_hallucination | 64 |
| terminal_punctuation | 18 |
| sentence_cased | 11 |
| no_fillers | 8 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 33 | 342 | 68 | 54 | 159 | 169 |
| total | 200 | 33 | 342 | 68 | 54 | 159 | 169 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0755 |
| Verbatim WER (macro) | 0.0812 |
| Entity accuracy | 0.7000 |

