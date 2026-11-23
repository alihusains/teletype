# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792751956
- **Model:** parakeet-tdt-v3-q4
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0394 |
| Macro WER | 0.0475 |
| Mean polish score | 69.9 |
| ASR p50 / p95 (ms) | 126 / 313 |
| Total p50 / p95 (ms) | 126 / 313 |
| Model load (ms) | 132 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 3505 | 138 | 0.0394 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-other-0022 | 0.5714 | 3 | 0 | 1 | asked zarathustra vehemently what doest thou here | I said Twistra vehemently, What doest thou hear? |
| librispeech-other-0173 | 0.4000 | 0 | 0 | 2 | no wait another half hour | No, wait another and a half hour. |
| librispeech-other-0035 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | Where are you bound for, young man? |
| librispeech-other-0138 | 0.3333 | 1 | 0 | 0 | this frees woman | This frees women. |
| librispeech-other-0080 | 0.3125 | 4 | 1 | 0 | will yo' alls please speak a lil louder he holler down the c… | Will y'all please speak a little louder? he hollered down th… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 66 |
| no_hallucination | 65 |
| terminal_punctuation | 20 |
| sentence_cased | 19 |
| no_fillers | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 62 | 482 | 147 | 126 | 313 | 409 |
| total | 200 | 62 | 482 | 147 | 126 | 313 | 409 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0387 |
| Verbatim WER (macro) | 0.0465 |
| Entity accuracy | 1.0000 |

