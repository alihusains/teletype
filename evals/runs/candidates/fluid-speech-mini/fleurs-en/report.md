# Teletype Eval Report

- **Git SHA:** 19b2767
- **Timestamp:** 1793034059
- **Model:** fluid-speech-mini
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 50 |
| Micro WER | 0.0667 |
| Macro WER | 0.0725 |
| Mean polish score | 50.3 |
| ASR p50 / p95 (ms) | 213 / 305 |
| Total p50 / p95 (ms) | 213 / 305 |
| Model load (ms) | 233 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| fleurs-en | 50 | 1095 | 73 | 0.0667 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| fleurs-en-0011 | 0.3333 | 3 | 0 | 2 | through the night between 150 and 200 copies were made now k… | For the night, between one fifty and two hundred copies were… |
| fleurs-en-0007 | 0.2857 | 3 | 0 | 3 | the aspect ratio of this format dividing by twelve to obtain… | The aspect ratio of this format, dividing by 12 to obtain th… |
| fleurs-en-0005 | 0.2500 | 1 | 1 | 0 | one bomb exploded outside the governor general's office | One bomb exploded outside the Governor-General's office. |
| fleurs-en-0020 | 0.2222 | 4 | 0 | 0 | but after losing the captain's wicket india only made 36 run… | But after losing the captains wicket, India only made 36 run… |
| fleurs-en-0047 | 0.1875 | 3 | 0 | 0 | the announcement was made after trump had a phone conversati… | The announcement was made after Trump had a phone conversati… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 28 |
| no_hallucination | 27 |
| bullets_when_listing | 1 |
| no_fillers | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 50 | 126 | 336 | 218 | 213 | 305 | 336 |
| total | 50 | 126 | 336 | 218 | 213 | 305 | 336 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0477 |
| Verbatim WER (macro) | 0.0487 |
| Entity accuracy | 0.7333 |

