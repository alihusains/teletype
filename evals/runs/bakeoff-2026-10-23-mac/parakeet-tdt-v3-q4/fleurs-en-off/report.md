# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792752092
- **Model:** parakeet-tdt-v3-q4
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 50 |
| Micro WER | 0.0658 |
| Macro WER | 0.0698 |
| Mean polish score | 47.5 |
| ASR p50 / p95 (ms) | 176 / 474 |
| Total p50 / p95 (ms) | 176 / 474 |
| Model load (ms) | 143 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| fleurs-en | 50 | 1095 | 72 | 0.0658 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| fleurs-en-0007 | 0.2857 | 3 | 0 | 3 | the aspect ratio of this format dividing by twelve to obtain… | The aspect ratio of this format, dividing by 12 to obtain th… |
| fleurs-en-0020 | 0.2778 | 5 | 0 | 0 | but after losing the captain's wicket india only made 36 run… | But after losing the captains wicket India only made 36 runs… |
| fleurs-en-0005 | 0.2500 | 1 | 1 | 0 | one bomb exploded outside the governor general's office | One bomb exploded outside the Governor-General's office. |
| fleurs-en-0023 | 0.2222 | 2 | 1 | 1 | throughout 1960s brzezinski worked for john f kennedy as his… | Throughout the 1960s, Kesinski worked for John F. Kennedy as… |
| fleurs-en-0033 | 0.2143 | 4 | 2 | 0 | during the struggle for independence organised by the mau mo… | During the struggle for independence organized by the Mao mo… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 30 |
| no_hallucination | 28 |
| bullets_when_listing | 1 |
| no_fillers | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 50 | 84 | 760 | 221 | 176 | 474 | 760 |
| total | 50 | 84 | 760 | 221 | 176 | 474 | 760 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0531 |
| Verbatim WER (macro) | 0.0550 |
| Entity accuracy | 0.8000 |

