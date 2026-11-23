# Teletype Eval Report

- **Git SHA:** 0e3ccb2
- **Timestamp:** 1792270368
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
| Micro WER | 0.1600 |
| Macro WER | 0.1906 |
| Mean polish score | 43.3 |
| ASR p50 / p95 (ms) | 296 / 967 |
| Total p50 / p95 (ms) | 296 / 967 |
| Model load (ms) | 558 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 263 | 0.1600 |
| snr-0db | 100 | 1644 | 263 | 0.1600 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-0db-0010 | 1.0000 | 4 | 3 | 0 | asked zarathustra vehemently what doest thou here | Ist to make it. |
| noise-0db-0025 | 1.0000 | 0 | 7 | 0 | the vil stuck fast in his throat |  |
| noise-0db-0033 | 1.0000 | 0 | 7 | 0 | but you are a jolly fellow too |  |
| noise-0db-0078 | 1.0000 | 0 | 4 | 0 | simply because this watch |  |
| noise-0db-0091 | 1.0000 | 11 | 6 | 0 | gurr glanced round to see if the men were looking and then s… | And then said that it has to be the kind of |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 65 |
| no_hallucination | 59 |
| terminal_punctuation | 14 |
| sentence_cased | 8 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 97 | 125 | 1654 | 415 | 296 | 967 | 1654 |
| total | 97 | 125 | 1654 | 415 | 296 | 967 | 1654 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1598 |
| Verbatim WER (macro) | 0.1924 |

