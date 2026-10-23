# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792775364
- **Model:** phonon2
- **Mode:** replay

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0979 |
| Macro WER | 0.1099 |
| Mean polish score | 50.5 |
| ASR p50 / p95 (ms) | 16 / 132 |
| Total p50 / p95 (ms) | 16 / 132 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 161 | 0.0979 |
| snr-0db | 100 | 1644 | 161 | 0.0979 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-0db-0025 | 0.7143 | 3 | 2 | 0 | the vil stuck fast in his throat | Deville stuck fastened the throat. |
| noise-0db-0047 | 0.6000 | 5 | 1 | 0 | oh no only a few details princess come this way | Oh no, only if you detailed. And this one. |
| noise-0db-0010 | 0.5714 | 4 | 0 | 0 | asked zarathustra vehemently what doest thou here | Ajá twist Ravemovski, What doest thou hear? |
| noise-0db-0033 | 0.4286 | 2 | 1 | 0 | but you are a jolly fellow too | But you're a jolly fellow change. |
| noise-0db-0089 | 0.4000 | 0 | 0 | 2 | no wait another half hour | No, wait another and a half hour. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 56 |
| no_hallucination | 52 |
| terminal_punctuation | 14 |
| sentence_cased | 13 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 8 | 86901 | 1722 | 16 | 132 | 82515 |
| total | 100 | 8 | 86901 | 1722 | 16 | 132 | 82515 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0965 |
| Verbatim WER (macro) | 0.1069 |

