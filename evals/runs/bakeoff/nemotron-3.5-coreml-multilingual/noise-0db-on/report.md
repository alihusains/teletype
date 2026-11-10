# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794279671
- **Model:** nemotron-3.5-coreml-multilingual
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.2232 |
| Macro WER | 0.2556 |
| Mean polish score | 35.0 |
| ASR p50 / p95 (ms) | 102 / 252 |
| Total p50 / p95 (ms) | 102 / 252 |
| Model load (ms) | 136 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 367 | 0.2232 |
| snr-0db | 100 | 1644 | 367 | 0.2232 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-0db-0010 | 1.0000 | 0 | 7 | 0 | asked zarathustra vehemently what doest thou here |  |
| noise-0db-0032 | 1.0000 | 5 | 0 | 0 | asked jean valjean fauchelevent replied | Action around first Levant re |
| noise-0db-0033 | 1.0000 | 0 | 7 | 0 | but you are a jolly fellow too |  |
| noise-0db-0042 | 1.0000 | 5 | 5 | 0 | sit down beside me and i'll tell you the story | ست عند سعادة هتطيب السب |
| noise-0db-0047 | 1.0000 | 5 | 5 | 0 | oh no only a few details princess come this way | Törnek ha miért figyelsz Anderson |

## Polish Failed Checks

| Check | Failures |
|---|---|
| terminal_punctuation | 99 |
| no_content_loss | 72 |
| no_hallucination | 67 |
| sentence_cased | 5 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 43 | 552 | 122 | 102 | 252 | 406 |
| total | 100 | 43 | 552 | 122 | 102 | 252 | 406 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.2226 |
| Verbatim WER (macro) | 0.2561 |

