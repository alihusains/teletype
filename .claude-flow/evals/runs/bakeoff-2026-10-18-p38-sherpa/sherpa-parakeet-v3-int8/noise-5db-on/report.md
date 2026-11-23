# Teletype Eval Report

- **Git SHA:** 0e3ccb2
- **Timestamp:** 1792270326
- **Model:** sherpa-parakeet-v3-int8
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.1052 |
| Macro WER | 0.1336 |
| Mean polish score | 51.1 |
| ASR p50 / p95 (ms) | 294 / 958 |
| Total p50 / p95 (ms) | 294 / 958 |
| Model load (ms) | 562 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 173 | 0.1052 |
| snr-5db | 100 | 1644 | 173 | 0.1052 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-5db-0010 | 1.0000 | 3 | 4 | 0 | asked zarathustra vehemently what doest thou here | I didn't know. |
| noise-5db-0078 | 1.0000 | 0 | 4 | 0 | simply because this watch |  |
| noise-5db-0025 | 0.8571 | 2 | 4 | 0 | the vil stuck fast in his throat | The feel sucks. |
| noise-5db-0033 | 0.7143 | 3 | 2 | 0 | but you are a jolly fellow too | But you're a Johnny Fetch. |
| noise-5db-0082 | 0.6364 | 2 | 4 | 1 | asked a worker last sunday you did it when the yunkers | After worker, last Sunday when the young girls |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 56 |
| no_hallucination | 51 |
| terminal_punctuation | 8 |
| sentence_cased | 7 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 99 | 126 | 1668 | 412 | 294 | 958 | 1668 |
| total | 99 | 126 | 1668 | 412 | 294 | 958 | 1668 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1031 |
| Verbatim WER (macro) | 0.1307 |

