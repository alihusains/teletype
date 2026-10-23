# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792752079
- **Model:** parakeet-tdt-v3-q4
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0858 |
| Macro WER | 0.1020 |
| Mean polish score | 52.7 |
| ASR p50 / p95 (ms) | 102 / 219 |
| Total p50 / p95 (ms) | 102 / 219 |
| Model load (ms) | 137 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 141 | 0.0858 |
| snr-0db | 100 | 1644 | 141 | 0.0858 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-0db-0010 | 0.7143 | 3 | 0 | 2 | asked zarathustra vehemently what doest thou here | I dear twist re vehemently. What doest thou hear? |
| noise-0db-0047 | 0.6000 | 6 | 0 | 0 | oh no only a few details princess come this way | Oh no, only if you dig out. On this one. |
| noise-0db-0066 | 0.5385 | 5 | 1 | 1 | he has tried that road and knows that it is the wrong road | He has prayed that for you and knows that the pieces are wro… |
| noise-0db-0025 | 0.4286 | 3 | 0 | 0 | the vil stuck fast in his throat | The wheel stuck fast in the surface. |
| noise-0db-0033 | 0.4286 | 2 | 1 | 0 | but you are a jolly fellow too | But you're a jolly fellow channel. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 52 |
| no_hallucination | 52 |
| sentence_cased | 8 |
| terminal_punctuation | 7 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 57 | 318 | 117 | 102 | 219 | 252 |
| total | 100 | 57 | 318 | 117 | 102 | 219 | 252 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0832 |
| Verbatim WER (macro) | 0.0985 |

