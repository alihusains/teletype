# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794279186
- **Model:** nemotron-3.5-coreml-multilingual
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0765 |
| Macro WER | 0.0861 |
| Mean polish score | 54.5 |
| ASR p50 / p95 (ms) | 97 / 267 |
| Total p50 / p95 (ms) | 97 / 267 |
| Model load (ms) | 135 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 3505 | 268 | 0.0765 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-other-0162 | 0.7500 | 2 | 0 | 1 | is platterbaff out yet | A splatter baff out yet |
| librispeech-other-0070 | 0.6000 | 3 | 0 | 0 | asked jean valjean fauchelevent replied | A Jean Varjean, Fauchelevon replied |
| librispeech-other-0022 | 0.5714 | 1 | 3 | 0 | asked zarathustra vehemently what doest thou here | What doest thou hear |
| librispeech-other-0031 | 0.5500 | 7 | 0 | 4 | the captain behaved perfectly well in this critical instant … | The captain behaved perfectly well in its credit for an inst… |
| librispeech-other-0110 | 0.4000 | 2 | 0 | 0 | the mirabelle was nearing tahiti | The Mirabel was nearing Tahi |

## Polish Failed Checks

| Check | Failures |
|---|---|
| terminal_punctuation | 197 |
| no_content_loss | 98 |
| no_hallucination | 95 |
| no_fillers | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 43 | 620 | 122 | 97 | 267 | 493 |
| total | 200 | 43 | 620 | 122 | 97 | 267 | 493 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0748 |
| Verbatim WER (macro) | 0.0848 |
| Entity accuracy | 1.0000 |

