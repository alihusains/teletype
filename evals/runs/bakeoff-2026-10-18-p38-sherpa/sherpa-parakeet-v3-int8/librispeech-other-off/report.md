# Teletype Eval Report

- **Git SHA:** 0e3ccb2
- **Timestamp:** 1792269979
- **Model:** sherpa-parakeet-v3-int8
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0474 |
| Macro WER | 0.0583 |
| Mean polish score | 66.8 |
| ASR p50 / p95 (ms) | 336 / 1013 |
| Total p50 / p95 (ms) | 336 / 1013 |
| Model load (ms) | 862 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 3505 | 166 | 0.0474 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-other-0022 | 0.7143 | 4 | 0 | 1 | asked zarathustra vehemently what doest thou here | As that twist travel. What doest thou hear? |
| librispeech-other-0138 | 0.6667 | 2 | 0 | 0 | this frees woman | This freeze women. |
| librispeech-other-0173 | 0.4000 | 0 | 0 | 2 | no wait another half hour | No, wait another and a half hour. |
| librispeech-other-0035 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | Where are you bound for, young man? |
| librispeech-other-0080 | 0.3125 | 4 | 1 | 0 | will yo' alls please speak a lil louder he holler down the c… | Will y'all please speak a little louder? he hollered down th… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 73 |
| no_hallucination | 72 |
| sentence_cased | 21 |
| terminal_punctuation | 13 |
| no_fillers | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 126 | 1674 | 432 | 336 | 1013 | 1339 |
| total | 200 | 126 | 1674 | 432 | 336 | 1013 | 1339 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0446 |
| Verbatim WER (macro) | 0.0547 |
| Entity accuracy | 0.6667 |

