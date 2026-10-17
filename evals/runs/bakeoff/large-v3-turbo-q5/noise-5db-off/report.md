# Teletype Eval Report

- **Git SHA:** d4b433b
- **Timestamp:** 1792230230
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
| Micro WER | 0.0736 |
| Macro WER | 0.0855 |
| Mean polish score | 58.4 |
| ASR p50 / p95 (ms) | 676 / 729 |
| Total p50 / p95 (ms) | 676 / 729 |
| Model load (ms) | 2256 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 121 | 0.0736 |
| snr-5db | 100 | 1644 | 121 | 0.0736 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-5db-0010 | 1.1429 | 5 | 0 | 3 | asked zarathustra vehemently what doest thou here | as a twist of humility what do we stop here |
| noise-5db-0089 | 0.4000 | 0 | 0 | 2 | no wait another half hour | No, wait another and a half hour. |
| noise-5db-0066 | 0.3846 | 5 | 0 | 0 | he has tried that road and knows that it is the wrong road | He has cried that way and knows that it is a drunk world. |
| noise-5db-0019 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | "Where are you bound for, young man?" |
| noise-5db-0037 | 0.3125 | 5 | 0 | 0 | will yo' alls please speak a lil louder he holler down the c… | "Will you all please speak a little louder?" he hollered dow… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 46 |
| no_hallucination | 44 |
| terminal_punctuation | 21 |
| sentence_cased | 16 |
| no_fillers | 3 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 644 | 814 | 682 | 676 | 729 | 740 |
| total | 100 | 644 | 814 | 682 | 676 | 729 | 740 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0639 |
| Verbatim WER (macro) | 0.0776 |

