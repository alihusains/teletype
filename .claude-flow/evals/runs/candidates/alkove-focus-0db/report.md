# Teletype Eval Report

- **Git SHA:** 4361547
- **Timestamp:** 1793039546
- **Model:** alkove-focus
- **Mode:** replay

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.3083 |
| Macro WER | 0.3113 |
| Mean polish score | 42.8 |
| ASR p50 / p95 (ms) | 402 / 1167 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| competing-speaker | 200 | 4126 | 1272 | 0.3083 |
| librispeech | 200 | 4126 | 1272 | 0.3083 |
| ssr-0db | 200 | 4126 | 1272 | 0.3083 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| competing-0db-0054 | 1.5556 | 9 | 0 | 5 | therefore don't talk to me about views and prospects | you know how much dropped I was of some part of what he said… |
| competing-0db-0154 | 1.5000 | 4 | 0 | 2 | it is annoyance then | The intention was to make things |
| competing-0db-0108 | 1.3333 | 3 | 0 | 1 | ocean reigned supreme | We see foreign countries. |
| competing-0db-0159 | 1.2500 | 8 | 0 | 2 | a sound of voices a flash of light | Why he meant to external things with his tail in |
| competing-0db-0049 | 1.2000 | 18 | 0 | 6 | the air and the earth are curiously mated and intermingled a… | And when he got the remaining went her way to the farthest o… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 131 |
| no_hallucination | 117 |
| terminal_punctuation | 60 |
| sentence_cased | 23 |
| no_fillers | 5 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 157 | 1659 | 514 | 402 | 1167 | 1417 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.3062 |
| Verbatim WER (macro) | 0.3081 |
| Entity accuracy | 0.4000 |

