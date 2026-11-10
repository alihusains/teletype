# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794277290
- **Model:** nemotron-3.5-asr-streaming-q4
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0293 |
| Macro WER | 0.0304 |
| Mean polish score | 71.4 |
| ASR p50 / p95 (ms) | 90 / 244 |
| Total p50 / p95 (ms) | 90 / 244 |
| Model load (ms) | 78 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 4126 | 121 | 0.0293 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-clean-0002 | 0.2500 | 1 | 1 | 0 | stuff it into you his belly counselled him | Stuffed into you his belly counselled him |
| librispeech-clean-0101 | 0.2000 | 1 | 0 | 0 | if i can get patients | If I can get patience. |
| librispeech-clean-0130 | 0.2000 | 1 | 0 | 0 | i boldly lighted my cheroot | I boldly lighted my cherute. |
| librispeech-clean-0048 | 0.1818 | 1 | 0 | 1 | oh no jasper i must go by my very own self | Oh no, Jap sir, I must go by my very own self. |
| librispeech-clean-0150 | 0.1667 | 2 | 1 | 1 | the weak kneed contingency must be strengthened and fortifie… | The weak need contingency must be strengthened and fortified… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 63 |
| no_hallucination | 63 |
| terminal_punctuation | 10 |
| no_fillers | 7 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 35 | 318 | 107 | 90 | 244 | 277 |
| total | 200 | 35 | 318 | 107 | 90 | 244 | 277 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0301 |
| Verbatim WER (macro) | 0.0315 |
| Entity accuracy | 0.9000 |

