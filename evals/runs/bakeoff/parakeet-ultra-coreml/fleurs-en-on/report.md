# Teletype Eval Report

- **Git SHA:** d4b433b
- **Timestamp:** 1792225927
- **Model:** parakeet-ultra-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 50 |
| Micro WER | 0.0621 |
| Macro WER | 0.0655 |
| Mean polish score | 49.4 |
| ASR p50 / p95 (ms) | 55 / 76 |
| Total p50 / p95 (ms) | 55 / 76 |
| Model load (ms) | 95 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| fleurs-en | 50 | 1095 | 68 | 0.0621 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| fleurs-en-0007 | 0.2857 | 3 | 0 | 3 | the aspect ratio of this format dividing by twelve to obtain… | The aspect ratio of this format, dividing by 12 to obtain th… |
| fleurs-en-0005 | 0.2500 | 1 | 1 | 0 | one bomb exploded outside the governor general's office | One bomb exploded outside the Governor-General's office. |
| fleurs-en-0020 | 0.2222 | 4 | 0 | 0 | but after losing the captain's wicket india only made 36 run… | But, after losing the captains wicket India, only made 36 ru… |
| fleurs-en-0047 | 0.1875 | 3 | 0 | 0 | the announcement was made after trump had a phone conversati… | The announcement was made after Trump had a phone conversati… |
| fleurs-en-0033 | 0.1786 | 5 | 0 | 0 | during the struggle for independence organised by the mau mo… | During the struggle for independence organized by the Mao mo… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 28 |
| no_hallucination | 28 |
| terminal_punctuation | 2 |
| bullets_when_listing | 1 |
| no_fillers | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 50 | 42 | 126 | 58 | 55 | 76 | 126 |
| total | 50 | 42 | 126 | 58 | 55 | 76 | 126 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0495 |
| Verbatim WER (macro) | 0.0510 |
| Entity accuracy | 0.8000 |

