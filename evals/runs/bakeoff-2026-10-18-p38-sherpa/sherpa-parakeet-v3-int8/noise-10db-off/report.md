# Teletype Eval Report

- **Git SHA:** 0e3ccb2
- **Timestamp:** 1792270193
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
| Micro WER | 0.0882 |
| Macro WER | 0.1100 |
| Mean polish score | 54.7 |
| ASR p50 / p95 (ms) | 302 / 953 |
| Total p50 / p95 (ms) | 302 / 953 |
| Model load (ms) | 563 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 145 | 0.0882 |
| snr-10db | 100 | 1644 | 145 | 0.0882 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-10db-0010 | 1.0000 | 4 | 3 | 0 | asked zarathustra vehemently what doest thou here | I there push travel. |
| noise-10db-0078 | 1.0000 | 0 | 4 | 0 | simply because this watch |  |
| noise-10db-0091 | 1.0000 | 0 | 17 | 0 | gurr glanced round to see if the men were looking and then s… |  |
| noise-10db-0025 | 0.4286 | 3 | 0 | 0 | the vil stuck fast in his throat | The veal stuck fast in the start. |
| noise-10db-0089 | 0.4000 | 0 | 0 | 2 | no wait another half hour | No, wait another and a half hour. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 51 |
| no_hallucination | 48 |
| sentence_cased | 10 |
| terminal_punctuation | 9 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 98 | 126 | 1667 | 411 | 302 | 953 | 1667 |
| total | 98 | 126 | 1667 | 411 | 302 | 953 | 1667 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0862 |
| Verbatim WER (macro) | 0.1069 |

