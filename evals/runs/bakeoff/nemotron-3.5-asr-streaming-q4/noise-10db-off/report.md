# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794277647
- **Model:** nemotron-3.5-asr-streaming-q4
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.1302 |
| Macro WER | 0.1549 |
| Mean polish score | 46.0 |
| ASR p50 / p95 (ms) | 68 / 177 |
| Total p50 / p95 (ms) | 68 / 177 |
| Model load (ms) | 66 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 214 | 0.1302 |
| snr-10db | 100 | 1644 | 214 | 0.1302 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-10db-0010 | 1.0000 | 0 | 7 | 0 | asked zarathustra vehemently what doest thou here |  |
| noise-10db-0026 | 1.0000 | 5 | 4 | 0 | general observations on preserves confectionary ices and des… | चैनल आप सर पर्सन, कन |
| noise-10db-0087 | 1.0000 | 0 | 5 | 0 | not you only a sham |  |
| noise-10db-0086 | 0.8000 | 3 | 0 | 1 | a telegram was brought in | I tell the gramas brought in. |
| noise-10db-0025 | 0.4286 | 3 | 0 | 0 | the vil stuck fast in his throat | The veil stuck fast in the surf. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 61 |
| no_hallucination | 58 |
| terminal_punctuation | 8 |
| sentence_cased | 3 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 98 | 37 | 327 | 87 | 68 | 177 | 327 |
| total | 98 | 37 | 327 | 87 | 68 | 177 | 327 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1273 |
| Verbatim WER (macro) | 0.1513 |

