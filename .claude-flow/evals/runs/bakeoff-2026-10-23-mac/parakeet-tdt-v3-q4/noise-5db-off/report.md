# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792752066
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
| Micro WER | 0.0602 |
| Macro WER | 0.0764 |
| Mean polish score | 61.1 |
| ASR p50 / p95 (ms) | 111 / 216 |
| Total p50 / p95 (ms) | 111 / 216 |
| Model load (ms) | 147 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 99 | 0.0602 |
| snr-5db | 100 | 1644 | 99 | 0.0602 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-5db-0010 | 0.5714 | 3 | 0 | 1 | asked zarathustra vehemently what doest thou here | I dear twister vehemently. What doest thou hear? |
| noise-5db-0025 | 0.4286 | 3 | 0 | 0 | the vil stuck fast in his throat | The veal stuck fast in the surface. |
| noise-5db-0033 | 0.4286 | 2 | 1 | 0 | but you are a jolly fellow too | But you're a jolly fellow chip. |
| noise-5db-0066 | 0.3846 | 4 | 1 | 0 | he has tried that road and knows that it is the wrong road | He has cried actually and knows that it is a wrong word. |
| noise-5db-0082 | 0.3636 | 3 | 0 | 1 | asked a worker last sunday you did it when the yunkers | Ask the worker, last Sunday you did it when the young girls |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 43 |
| no_hallucination | 42 |
| terminal_punctuation | 10 |
| sentence_cased | 9 |
| no_fillers | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 65 | 337 | 125 | 111 | 216 | 325 |
| total | 100 | 65 | 337 | 125 | 111 | 216 | 325 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0579 |
| Verbatim WER (macro) | 0.0729 |

