# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792751916
- **Model:** parakeet-v3-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0845 |
| Macro WER | 0.1059 |
| Mean polish score | 52.3 |
| ASR p50 / p95 (ms) | 42 / 80 |
| Total p50 / p95 (ms) | 42 / 80 |
| Model load (ms) | 88 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 139 | 0.0845 |
| snr-0db | 100 | 1644 | 139 | 0.0845 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-0db-0008 | 1.0000 | 0 | 0 | 3 | and also these | And also these are the two. |
| noise-0db-0066 | 0.7692 | 7 | 3 | 0 | he has tried that road and knows that it is the wrong road | Yes, very good. And those that he's a wrong one. |
| noise-0db-0025 | 0.5714 | 2 | 2 | 0 | the vil stuck fast in his throat | The real stuck fast start. |
| noise-0db-0010 | 0.4286 | 3 | 0 | 0 | asked zarathustra vehemently what doest thou here | Aja Twistra vehemently, what doest thou hear? |
| noise-0db-0033 | 0.4286 | 1 | 2 | 0 | but you are a jolly fellow too | But you're a jolly fellow. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 53 |
| no_hallucination | 52 |
| sentence_cased | 9 |
| terminal_punctuation | 4 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 33 | 116 | 47 | 42 | 80 | 108 |
| total | 100 | 33 | 116 | 47 | 42 | 80 | 108 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0808 |
| Verbatim WER (macro) | 0.1013 |

