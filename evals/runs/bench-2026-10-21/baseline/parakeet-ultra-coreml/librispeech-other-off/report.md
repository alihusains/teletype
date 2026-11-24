# Teletype Eval Report

- **Git SHA:** cee77e6
- **Timestamp:** 1792609484
- **Model:** parakeet-ultra-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0379 |
| Macro WER | 0.0441 |
| Mean polish score | 69.2 |
| ASR p50 / p95 (ms) | 47 / 103 |
| Total p50 / p95 (ms) | 47 / 103 |
| Model load (ms) | 105 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 3505 | 133 | 0.0379 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-other-0173 | 0.4000 | 0 | 0 | 2 | no wait another half hour | No, wait another and a half hour. |
| librispeech-other-0035 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | Where are you bound for, young man? |
| librispeech-other-0138 | 0.3333 | 1 | 0 | 0 | this frees woman | This frees women. |
| librispeech-other-0080 | 0.3125 | 5 | 0 | 0 | will yo' alls please speak a lil louder he holler down the c… | Will you all please speak a little louder? he hollered down … |
| librispeech-other-0022 | 0.2857 | 1 | 0 | 1 | asked zarathustra vehemently what doest thou here | asked the twistra vehemently, What doest thou here? |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 68 |
| no_hallucination | 66 |
| sentence_cased | 23 |
| terminal_punctuation | 18 |
| no_fillers | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 33 | 247 | 53 | 47 | 103 | 118 |
| total | 200 | 33 | 247 | 53 | 47 | 103 | 118 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0356 |
| Verbatim WER (macro) | 0.0420 |
| Entity accuracy | 1.0000 |

