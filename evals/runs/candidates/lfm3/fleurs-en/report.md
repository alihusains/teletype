# Teletype Eval Report

- **Git SHA:** fd63811
- **Timestamp:** 1793203438
- **Model:** lfm3
- **Mode:** replay

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 50 |
| Micro WER | 0.0511 |
| Macro WER | 0.0542 |
| Mean polish score | 59.1 |
| ASR p50 / p95 (ms) | 433 / 652 |
| Total p50 / p95 (ms) | 433 / 652 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| fleurs-en | 50 | 1095 | 56 | 0.0511 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| fleurs-en-0022 | 0.2941 | 3 | 0 | 2 | these are sometimes-crowded family beaches with a good range… | These are sometimes crowded family beaches with a good range… |
| fleurs-en-0007 | 0.2857 | 3 | 0 | 3 | the aspect ratio of this format dividing by twelve to obtain… | The aspect ratio of this format, dividing by 12 to obtain th… |
| fleurs-en-0035 | 0.2222 | 3 | 0 | 1 | with kundalini yoga the kundalini energy enlightenment energ… | With Kudali yoga, the Kudali energy and lightment energy is … |
| fleurs-en-0033 | 0.1786 | 5 | 0 | 0 | during the struggle for independence organised by the mau mo… | During the struggle for independence organized by the Mao mo… |
| fleurs-en-0020 | 0.1667 | 3 | 0 | 0 | but after losing the captain's wicket india only made 36 run… | But, after losing the captains wicket, India only made 36 ru… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 24 |
| no_hallucination | 21 |
| bullets_when_listing | 1 |
| no_fillers | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 50 | 229 | 694 | 440 | 433 | 652 | 694 |
| total | 50 | 229 | 694 | 440 | 433 | 652 | 694 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0423 |
| Verbatim WER (macro) | 0.0461 |
| Entity accuracy | 0.9333 |

