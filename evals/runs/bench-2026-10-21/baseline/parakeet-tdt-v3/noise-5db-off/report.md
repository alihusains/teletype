# Teletype Eval Report

- **Git SHA:** cee77e6
- **Timestamp:** 1792632634
- **Model:** parakeet-tdt-v3
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0590 |
| Macro WER | 0.0733 |
| Mean polish score | 62.9 |
| ASR p50 / p95 (ms) | 101 / 222 |
| Total p50 / p95 (ms) | 101 / 222 |
| Model load (ms) | 214 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 97 | 0.0590 |
| snr-5db | 100 | 1644 | 97 | 0.0590 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-5db-0025 | 0.7143 | 4 | 1 | 0 | the vil stuck fast in his throat | The veal stuck fastened the surface. |
| noise-5db-0010 | 0.5714 | 3 | 0 | 1 | asked zarathustra vehemently what doest thou here | I said twistra vehemently, What doest thou hear? |
| noise-5db-0033 | 0.4286 | 2 | 1 | 0 | but you are a jolly fellow too | But you're a jolly fellow chip. |
| noise-5db-0019 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | Where are you bound for, young man? |
| noise-5db-0037 | 0.3125 | 4 | 1 | 0 | will yo' alls please speak a lil louder he holler down the c… | Will y'all please speak a little louder? he hollered down th… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 42 |
| no_hallucination | 39 |
| terminal_punctuation | 10 |
| sentence_cased | 9 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 56 | 317 | 118 | 101 | 222 | 247 |
| total | 100 | 56 | 317 | 118 | 101 | 222 | 247 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0567 |
| Verbatim WER (macro) | 0.0698 |

