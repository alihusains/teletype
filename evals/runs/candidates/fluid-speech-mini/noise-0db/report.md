# Teletype Eval Report

- **Git SHA:** 19b2767
- **Timestamp:** 1793033913
- **Model:** fluid-speech-mini
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0821 |
| Macro WER | 0.0977 |
| Mean polish score | 55.6 |
| ASR p50 / p95 (ms) | 158 / 380 |
| Total p50 / p95 (ms) | 158 / 380 |
| Model load (ms) | 274 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 135 | 0.0821 |
| snr-0db | 100 | 1644 | 135 | 0.0821 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-0db-0025 | 0.7143 | 2 | 3 | 0 | the vil stuck fast in his throat | The veil struck fast. |
| noise-0db-0033 | 0.5714 | 2 | 2 | 0 | but you are a jolly fellow too | But you're a jolly fetch. |
| noise-0db-0066 | 0.5385 | 7 | 0 | 0 | he has tried that road and knows that it is the wrong road | He has pride and food, and knows that he kisses a wrong word… |
| noise-0db-0089 | 0.4000 | 0 | 0 | 2 | no wait another half hour | No, wait another and a half hour. |
| noise-0db-0037 | 0.3750 | 6 | 0 | 0 | will yo' alls please speak a lil louder he holler down the c… | Will you all please speak a little louder? he hollered out t… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 50 |
| no_hallucination | 47 |
| sentence_cased | 12 |
| terminal_punctuation | 10 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 81 | 668 | 192 | 158 | 380 | 608 |
| total | 100 | 81 | 668 | 192 | 158 | 380 | 608 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0790 |
| Verbatim WER (macro) | 0.0938 |

