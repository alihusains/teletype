# Teletype Eval Report

- **Git SHA:** d4b433b
- **Timestamp:** 1792230147
- **Model:** large-v3-turbo-q5
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0687 |
| Macro WER | 0.0812 |
| Mean polish score | 62.5 |
| ASR p50 / p95 (ms) | 677 / 747 |
| Total p50 / p95 (ms) | 677 / 747 |
| Model load (ms) | 2248 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 113 | 0.0687 |
| snr-10db | 100 | 1644 | 113 | 0.0687 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-10db-0010 | 1.1429 | 5 | 0 | 3 | asked zarathustra vehemently what doest thou here | as a twist of humility what do we stop here |
| noise-10db-0033 | 0.5714 | 2 | 1 | 1 | but you are a jolly fellow too | But you're a jolly fellow to you. |
| noise-10db-0081 | 0.4286 | 3 | 0 | 0 | that was hers went on the jeweler | "'That was hers,' went on the juror. |
| noise-10db-0089 | 0.4000 | 0 | 0 | 2 | no wait another half hour | No, wait another and a half hour. |
| noise-10db-0019 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | "Where are you bound for, young man?" |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 41 |
| no_hallucination | 40 |
| terminal_punctuation | 23 |
| sentence_cased | 17 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 648 | 792 | 687 | 677 | 747 | 767 |
| total | 100 | 648 | 792 | 687 | 677 | 747 | 767 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0579 |
| Verbatim WER (macro) | 0.0705 |

