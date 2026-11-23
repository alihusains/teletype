# Teletype Eval Report

- **Git SHA:** fa62b6d
- **Timestamp:** 1792271274
- **Model:** sherpa-parakeet-v2-int8
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 50 |
| Micro WER | 0.0858 |
| Macro WER | 0.0882 |
| Mean polish score | 49.3 |
| ASR p50 / p95 (ms) | 583 / 850 |
| Total p50 / p95 (ms) | 583 / 850 |
| Model load (ms) | 553 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| fleurs-en | 50 | 1095 | 94 | 0.0858 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| fleurs-en-0011 | 0.4667 | 5 | 0 | 2 | through the night between 150 and 200 copies were made now k… | With a night, between one fifty and two hundred copies were … |
| fleurs-en-0033 | 0.3929 | 9 | 1 | 1 | during the struggle for independence organised by the mau mo… | During the struggle for independence, organized by the Maoma… |
| fleurs-en-0020 | 0.2778 | 5 | 0 | 0 | but after losing the captain's wicket india only made 36 run… | But, after losing the captains wicked India only made 36 run… |
| fleurs-en-0023 | 0.2778 | 3 | 1 | 1 | throughout 1960s brzezinski worked for john f kennedy as his… | Throughout the 1960s, Bazinski worked for John F. Kenny as h… |
| fleurs-en-0005 | 0.2500 | 1 | 1 | 0 | one bomb exploded outside the governor general's office | One bomb exploded outside the Governor-General's office. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 29 |
| no_hallucination | 27 |
| bullets_when_listing | 1 |
| no_fillers | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 50 | 266 | 1039 | 606 | 583 | 850 | 1039 |
| total | 50 | 266 | 1039 | 606 | 583 | 850 | 1039 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0603 |
| Verbatim WER (macro) | 0.0610 |
| Entity accuracy | 0.8000 |

