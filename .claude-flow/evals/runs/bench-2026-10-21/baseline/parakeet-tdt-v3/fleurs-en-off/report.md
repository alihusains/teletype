# Teletype Eval Report

- **Git SHA:** cee77e6
- **Timestamp:** 1792632665
- **Model:** parakeet-tdt-v3
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 50 |
| Micro WER | 0.0648 |
| Macro WER | 0.0691 |
| Mean polish score | 52.1 |
| ASR p50 / p95 (ms) | 182 / 353 |
| Total p50 / p95 (ms) | 182 / 353 |
| Model load (ms) | 205 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| fleurs-en | 50 | 1095 | 71 | 0.0648 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| fleurs-en-0007 | 0.2857 | 3 | 0 | 3 | the aspect ratio of this format dividing by twelve to obtain… | The aspect ratio of this format, dividing by 12 to obtain th… |
| fleurs-en-0020 | 0.2778 | 5 | 0 | 0 | but after losing the captain's wicket india only made 36 run… | But after losing the caps' wicket India only made 36 runs, l… |
| fleurs-en-0005 | 0.2500 | 1 | 1 | 0 | one bomb exploded outside the governor general's office | One bomb exploded outside the Governor-General's office. |
| fleurs-en-0047 | 0.2500 | 3 | 0 | 1 | the announcement was made after trump had a phone conversati… | The announcement was made after Trump had a phone conversati… |
| fleurs-en-0023 | 0.2222 | 2 | 1 | 1 | throughout 1960s brzezinski worked for john f kennedy as his… | Throughout the 1960s, Bresinski worked for John F. Kennedy a… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 27 |
| no_hallucination | 26 |
| bullets_when_listing | 1 |
| no_fillers | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 50 | 92 | 393 | 198 | 182 | 353 | 393 |
| total | 50 | 92 | 393 | 198 | 182 | 353 | 393 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0549 |
| Verbatim WER (macro) | 0.0565 |
| Entity accuracy | 0.8000 |

