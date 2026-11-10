# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794277707
- **Model:** nemotron-3.5-asr-streaming-q4
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 50 |
| Micro WER | 0.1370 |
| Macro WER | 0.1325 |
| Mean polish score | 38.9 |
| ASR p50 / p95 (ms) | 113 / 184 |
| Total p50 / p95 (ms) | 113 / 184 |
| Model load (ms) | 70 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| fleurs-en | 50 | 1095 | 150 | 0.1370 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| fleurs-en-0028 | 1.0000 | 0 | 25 | 0 | the number of people present was so large that it was not po… |  |
| fleurs-en-0039 | 0.6364 | 0 | 14 | 0 | if you're not used to driving on country roads keep your wit… | Steep grades, narrow lanes, and sharp curves predominate. |
| fleurs-en-0011 | 0.3333 | 3 | 0 | 2 | through the night between 150 and 200 copies were made now k… | To the night between one fifty and two hundred copies were m… |
| fleurs-en-0020 | 0.3333 | 5 | 0 | 1 | but after losing the captain's wicket india only made 36 run… | But after losing the captain's wicked India only made thirty… |
| fleurs-en-0049 | 0.3333 | 4 | 0 | 0 | lions are the most social cats living in large groups called… | I enter the most social cats living in large groups calls pr… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 35 |
| no_hallucination | 32 |
| terminal_punctuation | 9 |
| bullets_when_listing | 1 |
| no_fillers | 1 |
| sentence_cased | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 49 | 64 | 230 | 121 | 113 | 184 | 230 |
| total | 49 | 64 | 230 | 121 | 113 | 184 | 230 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1053 |
| Verbatim WER (macro) | 0.1034 |
| Entity accuracy | 0.6000 |

