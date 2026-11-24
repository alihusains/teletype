# Teletype Eval Report

- **Git SHA:** 19b2767
- **Timestamp:** 1793036484
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
| Micro WER | 0.4457 |
| Macro WER | 0.4443 |
| Mean polish score | 39.9 |
| ASR p50 / p95 (ms) | 169 / 443 |
| Total p50 / p95 (ms) | 169 / 443 |
| Model load (ms) | 202 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| competing-speaker | 200 | 4126 | 1839 | 0.4457 |
| librispeech | 200 | 4126 | 1839 | 0.4457 |
| ssr-0db | 200 | 4126 | 1839 | 0.4457 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| competing-0db-0032 | 1.5000 | 3 | 0 | 3 | no sir certainly not | No such its intention to his orders. |
| competing-0db-0154 | 1.5000 | 4 | 0 | 2 | it is annoyance then | The intention was to make things |
| competing-0db-0063 | 1.4667 | 14 | 0 | 8 | the influence with the timaeus has exercised upon posterity … | experimenting with his tail and fins with such enthusiasm as… |
| competing-0db-0021 | 1.4286 | 7 | 0 | 3 | ojo examined this curious contrivance with wonder | Ram showed his white teeth as he burst out long. |
| competing-0db-0108 | 1.3333 | 3 | 0 | 1 | ocean reigned supreme | We see foreign countries. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 138 |
| no_hallucination | 121 |
| terminal_punctuation | 82 |
| sentence_cased | 36 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 196 | 92 | 631 | 207 | 169 | 443 | 518 |
| total | 196 | 92 | 631 | 207 | 169 | 443 | 518 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.4458 |
| Verbatim WER (macro) | 0.4435 |
| Entity accuracy | 0.4000 |

