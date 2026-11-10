# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794277683
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
| Micro WER | 0.2257 |
| Macro WER | 0.2532 |
| Mean polish score | 36.9 |
| ASR p50 / p95 (ms) | 63 / 173 |
| Total p50 / p95 (ms) | 63 / 173 |
| Model load (ms) | 65 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 371 | 0.2257 |
| snr-0db | 100 | 1644 | 371 | 0.2257 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-0db-0010 | 1.0000 | 4 | 3 | 0 | asked zarathustra vehemently what doest thou here | वट डू स्टव हेयर |
| noise-0db-0026 | 1.0000 | 5 | 4 | 0 | general observations on preserves confectionary ices and des… | चलो आप ऑन प्रजर्व्स, कन्क्शरी, |
| noise-0db-0027 | 1.0000 | 3 | 4 | 0 | yes that i have said the youth | يا سنعاد سبيل |
| noise-0db-0033 | 1.0000 | 0 | 7 | 0 | but you are a jolly fellow too |  |
| noise-0db-0042 | 1.0000 | 4 | 6 | 0 | sit down beside me and i'll tell you the story | ستان بسعر المطيب السبت. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 70 |
| no_hallucination | 69 |
| sentence_cased | 8 |
| terminal_punctuation | 8 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 98 | 37 | 323 | 86 | 63 | 173 | 323 |
| total | 98 | 37 | 323 | 86 | 63 | 173 | 323 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.2232 |
| Verbatim WER (macro) | 0.2519 |

