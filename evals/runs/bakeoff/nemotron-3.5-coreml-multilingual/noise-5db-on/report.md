# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794279646
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
| Micro WER | 0.1715 |
| Macro WER | 0.2083 |
| Mean polish score | 40.5 |
| ASR p50 / p95 (ms) | 102 / 251 |
| Total p50 / p95 (ms) | 102 / 251 |
| Model load (ms) | 134 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 282 | 0.1715 |
| snr-5db | 100 | 1644 | 282 | 0.1715 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-5db-0032 | 1.2000 | 5 | 0 | 1 | asked jean valjean fauchelevent replied | As gens argents First Levant repli |
| noise-5db-0026 | 1.0000 | 7 | 2 | 0 | general observations on preserves confectionary ices and des… | जनरल ऑब्जर्वेशन्स ऑन पर्सेव, कन इस, डेस |
| noise-5db-0066 | 1.0000 | 0 | 13 | 0 | he has tried that road and knows that it is the wrong road |  |
| noise-5db-0087 | 1.0000 | 4 | 1 | 0 | not you only a sham | Naju, um mie šē |
| noise-5db-0010 | 0.8571 | 4 | 2 | 0 | asked zarathustra vehemently what doest thou here | What do I stau here |

## Polish Failed Checks

| Check | Failures |
|---|---|
| terminal_punctuation | 99 |
| no_content_loss | 65 |
| no_hallucination | 62 |
| no_fillers | 4 |
| sentence_cased | 3 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 42 | 715 | 124 | 102 | 251 | 477 |
| total | 100 | 42 | 715 | 124 | 102 | 251 | 477 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1713 |
| Verbatim WER (macro) | 0.2091 |

