# Teletype Eval Report

- **Git SHA:** 19b2767
- **Timestamp:** 1793036724
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
| Micro WER | 0.0238 |
| Macro WER | 0.0320 |
| Mean polish score | 77.0 |
| ASR p50 / p95 (ms) | 54 / 160 |
| Total p50 / p95 (ms) | 54 / 160 |
| Model load (ms) | 119 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| competing-speaker | 200 | 4126 | 98 | 0.0238 |
| librispeech | 200 | 4126 | 98 | 0.0238 |
| ssr-10db | 200 | 4126 | 98 | 0.0238 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| competing-10db-0100 | 0.7500 | 0 | 0 | 3 | i will practice it | I will practice it. She put her. |
| competing-10db-0174 | 0.3333 | 1 | 2 | 0 | this was at the march election eighteen fifty five | This was at the March election, 1855. |
| competing-10db-0031 | 0.2857 | 1 | 1 | 0 | the foul mouthed fellow at the top | The foul-mouthed fellow at the top. |
| competing-10db-0173 | 0.2857 | 1 | 1 | 0 | the bogus legislature numbered thirty six members | The bogus legislature numbered thirty-six members. |
| competing-10db-0035 | 0.2222 | 2 | 0 | 2 | goobers don't grow on the tops of vines but underground on t… | Gubels don't grow on the tops of vines, but on the ground, o… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 50 |
| no_hallucination | 50 |
| sentence_cased | 16 |
| terminal_punctuation | 16 |
| no_fillers | 7 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 35 | 170 | 66 | 54 | 160 | 165 |
| total | 200 | 35 | 170 | 66 | 54 | 160 | 165 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0209 |
| Verbatim WER (macro) | 0.0266 |
| Entity accuracy | 0.9000 |

