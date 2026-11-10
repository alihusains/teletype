# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794279683
- **Model:** nemotron-3.5-coreml-multilingual
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 50 |
| Micro WER | 0.1388 |
| Macro WER | 0.1410 |
| Mean polish score | 33.6 |
| ASR p50 / p95 (ms) | 154 / 232 |
| Total p50 / p95 (ms) | 154 / 232 |
| Model load (ms) | 133 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| fleurs-en | 50 | 1095 | 152 | 0.1388 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| fleurs-en-0020 | 0.9444 | 0 | 17 | 0 | but after losing the captain's wicket india only made 36 run… | But |
| fleurs-en-0049 | 0.6667 | 2 | 6 | 0 | lions are the most social cats living in large groups called… | Living in large groups calls brights |
| fleurs-en-0039 | 0.6364 | 0 | 14 | 0 | if you're not used to driving on country roads keep your wit… | Steep grades, narrow lanes, and sharp curves predominate |
| fleurs-en-0011 | 0.4000 | 2 | 2 | 2 | through the night between 150 and 200 copies were made now k… | The night between one fifty and two hundred copies were now … |
| fleurs-en-0047 | 0.3125 | 5 | 0 | 0 | the announcement was made after trump had a phone conversati… | The announcement was made after Trump had a phone conversati… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| terminal_punctuation | 48 |
| no_content_loss | 38 |
| no_hallucination | 33 |
| bullets_when_listing | 1 |
| no_fillers | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 50 | 75 | 331 | 164 | 154 | 232 | 331 |
| total | 50 | 75 | 331 | 164 | 154 | 232 | 331 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1089 |
| Verbatim WER (macro) | 0.1133 |
| Entity accuracy | 0.5333 |

