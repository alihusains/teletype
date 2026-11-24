# Teletype Eval Report

- **Git SHA:** 19b2767
- **Timestamp:** 1793033858
- **Model:** fluid-speech-mini
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0417 |
| Macro WER | 0.0478 |
| Mean polish score | 67.2 |
| ASR p50 / p95 (ms) | 171 / 451 |
| Total p50 / p95 (ms) | 171 / 451 |
| Model load (ms) | 209 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 3505 | 146 | 0.0417 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-other-0173 | 0.4000 | 0 | 0 | 2 | no wait another half hour | No, wait another and a half hour. |
| librispeech-other-0035 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | Where are you bound for, young man? |
| librispeech-other-0138 | 0.3333 | 1 | 0 | 0 | this frees woman | This frees women, |
| librispeech-other-0080 | 0.3125 | 5 | 0 | 0 | will yo' alls please speak a lil louder he holler down the c… | Will you all please speak a little louder? he hollered down … |
| librispeech-other-0005 | 0.2857 | 1 | 1 | 0 | improvident and self indulgent were his words | Improvident and self-indulgent were his words. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 72 |
| no_hallucination | 71 |
| sentence_cased | 24 |
| terminal_punctuation | 21 |
| no_fillers | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 83 | 884 | 212 | 171 | 451 | 667 |
| total | 200 | 83 | 884 | 212 | 171 | 451 | 667 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0378 |
| Verbatim WER (macro) | 0.0434 |
| Entity accuracy | 1.0000 |

