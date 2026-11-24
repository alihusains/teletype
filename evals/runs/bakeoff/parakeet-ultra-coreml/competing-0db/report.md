# Teletype Eval Report

- **Git SHA:** 19b2767
- **Timestamp:** 1793036662
- **Model:** parakeet-ultra-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.4358 |
| Macro WER | 0.4287 |
| Mean polish score | 37.7 |
| ASR p50 / p95 (ms) | 54 / 162 |
| Total p50 / p95 (ms) | 54 / 162 |
| Model load (ms) | 11116 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| competing-speaker | 200 | 4126 | 1798 | 0.4358 |
| librispeech | 200 | 4126 | 1798 | 0.4358 |
| ssr-0db | 200 | 4126 | 1798 | 0.4358 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| competing-0db-0052 | 2.2000 | 5 | 0 | 6 | the horizon seems extremely distant | Didn't I say so? Worst luck to my line, said Sancho. |
| competing-0db-0137 | 1.5556 | 6 | 0 | 8 | nay nay lording answered warrenton with a half laugh | Of which he made a compound, mixing them up, boy, answered W… |
| competing-0db-0154 | 1.5000 | 4 | 0 | 2 | it is annoyance then | Their intention was to make things. |
| competing-0db-0063 | 1.4667 | 13 | 0 | 9 | the influence with the timaeus has exercised upon posterity … | experimenting with his tail and fins with such enthusiasm an… |
| competing-0db-0054 | 1.4444 | 9 | 0 | 4 | therefore don't talk to me about views and prospects | You know how much shocked I was some part of what he said. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 141 |
| no_hallucination | 131 |
| terminal_punctuation | 38 |
| sentence_cased | 23 |
| no_fillers | 5 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 34 | 366 | 68 | 54 | 162 | 169 |
| total | 200 | 34 | 366 | 68 | 54 | 162 | 169 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.4330 |
| Verbatim WER (macro) | 0.4228 |
| Entity accuracy | 0.4000 |

