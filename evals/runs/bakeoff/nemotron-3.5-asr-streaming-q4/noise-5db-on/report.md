# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794277674
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
| Micro WER | 0.1575 |
| Macro WER | 0.1873 |
| Mean polish score | 42.8 |
| ASR p50 / p95 (ms) | 65 / 193 |
| Total p50 / p95 (ms) | 65 / 193 |
| Model load (ms) | 64 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 259 | 0.1575 |
| snr-5db | 100 | 1644 | 259 | 0.1575 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-5db-0026 | 1.1111 | 9 | 0 | 1 | general observations on preserves confectionary ices and des… | चलो आप सर ऑन प्रेजर्स, कन्शर्ियरिंग, इसेस, एंड डर्ट डेशेज. |
| noise-5db-0010 | 1.0000 | 0 | 7 | 0 | asked zarathustra vehemently what doest thou here |  |
| noise-5db-0033 | 1.0000 | 0 | 7 | 0 | but you are a jolly fellow too |  |
| noise-5db-0042 | 1.0000 | 5 | 5 | 0 | sit down beside me and i'll tell you the story | september ساعدين عود طيب السوري. |
| noise-5db-0087 | 1.0000 | 5 | 0 | 0 | not you only a sham | Na jo, um ji šena. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 64 |
| no_hallucination | 62 |
| sentence_cased | 5 |
| terminal_punctuation | 5 |
| no_fillers | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 98 | 36 | 319 | 88 | 65 | 193 | 319 |
| total | 98 | 36 | 319 | 88 | 65 | 193 | 319 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1592 |
| Verbatim WER (macro) | 0.1920 |

