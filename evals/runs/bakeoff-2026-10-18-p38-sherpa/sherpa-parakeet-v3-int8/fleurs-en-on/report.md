# Teletype Eval Report

- **Git SHA:** 0e3ccb2
- **Timestamp:** 1792270485
- **Model:** sherpa-parakeet-v3-int8
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 50 |
| Micro WER | 0.1288 |
| Macro WER | 0.1289 |
| Mean polish score | 46.5 |
| ASR p50 / p95 (ms) | 609 / 862 |
| Total p50 / p95 (ms) | 609 / 862 |
| Model load (ms) | 559 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| fleurs-en | 50 | 1095 | 141 | 0.1288 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| fleurs-en-0033 | 1.0000 | 0 | 28 | 0 | during the struggle for independence organised by the mau mo… |  |
| fleurs-en-0047 | 0.6250 | 4 | 0 | 6 | the announcement was made after trump had a phone conversati… | The announcement was made after Trump had a phone conversati… |
| fleurs-en-0011 | 0.6000 | 6 | 0 | 3 | through the night between 150 and 200 copies were made now k… | With a night between one fifty and two hundred copies were m… |
| fleurs-en-0022 | 0.3529 | 3 | 0 | 3 | these are sometimes-crowded family beaches with a good range… | These are sometimes crowded family beaches with a good range… |
| fleurs-en-0014 | 0.2895 | 9 | 0 | 2 | because the dinosaur feathers do not have a well-developed s… | Because the dinosaur feathers do not have a well developed s… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 30 |
| no_hallucination | 29 |
| no_fillers | 2 |
| bullets_when_listing | 1 |
| sentence_cased | 1 |
| terminal_punctuation | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 49 | 270 | 1036 | 614 | 609 | 862 | 1036 |
| total | 49 | 270 | 1036 | 614 | 609 | 862 | 1036 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1053 |
| Verbatim WER (macro) | 0.1042 |
| Entity accuracy | 0.8667 |

