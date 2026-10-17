# Teletype Eval Report

- **Git SHA:** d4b433b
- **Timestamp:** 1792230560
- **Model:** large-v3-turbo-q5
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 50 |
| Micro WER | 0.0484 |
| Macro WER | 0.0502 |
| Mean polish score | 60.1 |
| ASR p50 / p95 (ms) | 704 / 737 |
| Total p50 / p95 (ms) | 704 / 737 |
| Model load (ms) | 2304 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| fleurs-en | 50 | 1095 | 53 | 0.0484 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| fleurs-en-0007 | 0.2857 | 3 | 0 | 3 | the aspect ratio of this format dividing by twelve to obtain… | The aspect ratio of this format, dividing by 12 to obtain th… |
| fleurs-en-0009 | 0.2222 | 2 | 0 | 2 | if you only go ashore using shipboard excursions you will no… | If you only go ashore using ship or excursions you will not … |
| fleurs-en-0047 | 0.1875 | 2 | 0 | 1 | the announcement was made after trump had a phone conversati… | The announcement was made after Trump had a phone conversati… |
| fleurs-en-0022 | 0.1765 | 1 | 1 | 1 | these are sometimes-crowded family beaches with a good range… | These are sometimes crowded family beaches with a good range… |
| fleurs-en-0020 | 0.1111 | 2 | 0 | 0 | but after losing the captain's wicket india only made 36 run… | But, after losing the captain's wicket India only made 36 ru… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 23 |
| no_hallucination | 21 |
| bullets_when_listing | 1 |
| no_fillers | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 50 | 668 | 747 | 702 | 704 | 737 | 747 |
| total | 50 | 668 | 747 | 702 | 704 | 737 | 747 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0405 |
| Verbatim WER (macro) | 0.0423 |
| Entity accuracy | 0.8667 |

