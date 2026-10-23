# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792752051
- **Model:** parakeet-tdt-v3-q4
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0499 |
| Macro WER | 0.0615 |
| Mean polish score | 64.8 |
| ASR p50 / p95 (ms) | 117 / 283 |
| Total p50 / p95 (ms) | 117 / 283 |
| Model load (ms) | 143 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 82 | 0.0499 |
| snr-10db | 100 | 1644 | 82 | 0.0499 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-10db-0010 | 0.5714 | 3 | 0 | 1 | asked zarathustra vehemently what doest thou here | I said twistra vehemently What doest thou hear? |
| noise-10db-0033 | 0.4286 | 2 | 1 | 0 | but you are a jolly fellow too | But you're a jolly fellow cheap. |
| noise-10db-0082 | 0.3636 | 3 | 0 | 1 | asked a worker last sunday you did it when the yunkers | Ask the worker, last Sunday you did it when the young girls |
| noise-10db-0019 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | Where are you bound for, young man? |
| noise-10db-0037 | 0.3125 | 4 | 1 | 0 | will yo' alls please speak a lil louder he holler down the c… | Will y'all please speak a little louder? he hollered down th… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_hallucination | 39 |
| no_content_loss | 38 |
| sentence_cased | 11 |
| terminal_punctuation | 11 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 62 | 378 | 139 | 117 | 283 | 332 |
| total | 100 | 62 | 378 | 139 | 117 | 283 | 332 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0476 |
| Verbatim WER (macro) | 0.0581 |

