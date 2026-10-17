# Teletype Eval Report

- **Git SHA:** fa62b6d
- **Timestamp:** 1792271032
- **Model:** sherpa-parakeet-v2-int8
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0468 |
| Macro WER | 0.0608 |
| Mean polish score | 62.9 |
| ASR p50 / p95 (ms) | 287 / 935 |
| Total p50 / p95 (ms) | 287 / 935 |
| Model load (ms) | 549 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 77 | 0.0468 |
| snr-10db | 100 | 1644 | 77 | 0.0468 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-10db-0025 | 0.5714 | 3 | 1 | 0 | the vil stuck fast in his throat | Leville stuck fast in the soap. |
| noise-10db-0010 | 0.4286 | 3 | 0 | 0 | asked zarathustra vehemently what doest thou here | answered to Stravehami, what doest thou here? |
| noise-10db-0019 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | Where are you bound for, young man? |
| noise-10db-0037 | 0.3125 | 5 | 0 | 0 | will yo' alls please speak a lil louder he holler down the c… | Will you all please speak a little louder? he hollered down … |
| noise-10db-0029 | 0.2857 | 2 | 0 | 0 | die thou fafnir and then fafnir died | Die, thou Fafner and then Fafner died. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_hallucination | 42 |
| no_content_loss | 39 |
| terminal_punctuation | 16 |
| sentence_cased | 14 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 124 | 1634 | 405 | 287 | 935 | 1262 |
| total | 100 | 124 | 1634 | 405 | 287 | 935 | 1262 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0458 |
| Verbatim WER (macro) | 0.0601 |

