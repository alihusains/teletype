# Teletype Eval Report

- **Git SHA:** d4b433b
- **Timestamp:** 1792224493
- **Model:** parakeet-tdt-v3
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0785 |
| Macro WER | 0.0939 |
| Mean polish score | 53.1 |
| ASR p50 / p95 (ms) | 187 / 504 |
| Total p50 / p95 (ms) | 187 / 504 |
| Model load (ms) | 197 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 129 | 0.0785 |
| snr-0db | 100 | 1644 | 129 | 0.0785 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-0db-0066 | 0.5385 | 5 | 2 | 0 | he has tried that road and knows that it is the wrong road | He has pride completely and knows that he's a wrong one. |
| noise-0db-0010 | 0.4286 | 3 | 0 | 0 | asked zarathustra vehemently what doest thou here | Ija twistra vehemently. What doest thou hear? |
| noise-0db-0025 | 0.4286 | 3 | 0 | 0 | the vil stuck fast in his throat | The veil stuck fast in the surface. |
| noise-0db-0033 | 0.4286 | 2 | 1 | 0 | but you are a jolly fellow too | But you're a jolly fellow channel. |
| noise-0db-0047 | 0.4000 | 4 | 0 | 0 | oh no only a few details princess come this way | Oh no, only if you did that. Come this way. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 52 |
| no_hallucination | 51 |
| sentence_cased | 9 |
| terminal_punctuation | 9 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 108 | 631 | 236 | 187 | 504 | 627 |
| total | 100 | 108 | 631 | 236 | 187 | 504 | 627 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0754 |
| Verbatim WER (macro) | 0.0899 |

