# Teletype Eval Report

- **Git SHA:** d4b433b
- **Timestamp:** 1792225911
- **Model:** parakeet-ultra-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0742 |
| Macro WER | 0.0898 |
| Mean polish score | 55.6 |
| ASR p50 / p95 (ms) | 46 / 74 |
| Total p50 / p95 (ms) | 46 / 74 |
| Model load (ms) | 95 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 122 | 0.0742 |
| snr-0db | 100 | 1644 | 122 | 0.0742 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-0db-0047 | 0.6000 | 6 | 0 | 0 | oh no only a few details princess come this way | Oh no, only if you did that. And this one. |
| noise-0db-0070 | 0.4444 | 3 | 0 | 1 | failure seems to be the trademark of nature why | Failure seems to be the prey to not mistake. Why? |
| noise-0db-0010 | 0.4286 | 3 | 0 | 0 | asked zarathustra vehemently what doest thou here | Aja Twistra vehemently, What doest thou hear? |
| noise-0db-0025 | 0.4286 | 3 | 0 | 0 | the vil stuck fast in his throat | The veil stuck fast in the surface. |
| noise-0db-0033 | 0.4286 | 2 | 1 | 0 | but you are a jolly fellow too | But you're a jolly fellow chip. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 49 |
| no_hallucination | 48 |
| sentence_cased | 11 |
| terminal_punctuation | 10 |
| no_fillers | 3 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 35 | 118 | 51 | 46 | 74 | 115 |
| total | 100 | 35 | 118 | 51 | 46 | 74 | 115 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0718 |
| Verbatim WER (macro) | 0.0863 |

