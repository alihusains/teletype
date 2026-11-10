# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794277331
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
| Micro WER | 0.0793 |
| Macro WER | 0.0868 |
| Mean polish score | 55.5 |
| ASR p50 / p95 (ms) | 76 / 181 |
| Total p50 / p95 (ms) | 76 / 181 |
| Model load (ms) | 69 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 3505 | 278 | 0.0793 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-other-0162 | 0.7500 | 2 | 0 | 1 | is platterbaff out yet | It splatter bath out yet. |
| librispeech-other-0138 | 0.6667 | 2 | 0 | 0 | this frees woman | This freeze women. |
| librispeech-other-0163 | 0.6000 | 2 | 1 | 0 | a telegram was brought in | I telegramas brought in. |
| librispeech-other-0031 | 0.5500 | 8 | 0 | 3 | the captain behaved perfectly well in this critical instant … | The captain behaved perfectly well and its credit for an ins… |
| librispeech-other-0022 | 0.4286 | 3 | 0 | 0 | asked zarathustra vehemently what doest thou here | That twisted vehemently, what doest thou hear? |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 99 |
| no_hallucination | 98 |
| terminal_punctuation | 9 |
| no_fillers | 4 |
| sentence_cased | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 35 | 325 | 91 | 76 | 181 | 229 |
| total | 200 | 35 | 325 | 91 | 76 | 181 | 229 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0776 |
| Verbatim WER (macro) | 0.0853 |
| Entity accuracy | 1.0000 |

