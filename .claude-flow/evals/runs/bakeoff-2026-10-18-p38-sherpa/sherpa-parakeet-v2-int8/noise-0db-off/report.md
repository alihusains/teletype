# Teletype Eval Report

- **Git SHA:** fa62b6d
- **Timestamp:** 1792271158
- **Model:** sherpa-parakeet-v2-int8
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0833 |
| Macro WER | 0.1092 |
| Mean polish score | 49.3 |
| ASR p50 / p95 (ms) | 287 / 930 |
| Total p50 / p95 (ms) | 287 / 930 |
| Model load (ms) | 547 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 137 | 0.0833 |
| snr-0db | 100 | 1644 | 137 | 0.0833 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-0db-0025 | 1.0000 | 3 | 4 | 0 | the vil stuck fast in his throat | Reveal separate search. |
| noise-0db-0027 | 0.7143 | 3 | 2 | 0 | yes that i have said the youth | Yes, Dana has the bees. |
| noise-0db-0033 | 0.7143 | 3 | 2 | 0 | but you are a jolly fellow too | But you're a joint bench. |
| noise-0db-0091 | 0.5294 | 5 | 0 | 4 | gurr glanced round to see if the men were looking and then s… | Girl glanced round to see if the men were looking, and then … |
| noise-0db-0010 | 0.4286 | 3 | 0 | 0 | asked zarathustra vehemently what doest thou here | Argento is travelling. What doest thou here? |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 56 |
| no_hallucination | 55 |
| terminal_punctuation | 13 |
| sentence_cased | 11 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 124 | 1667 | 406 | 287 | 930 | 1249 |
| total | 100 | 124 | 1667 | 406 | 287 | 930 | 1249 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0814 |
| Verbatim WER (macro) | 0.1056 |

