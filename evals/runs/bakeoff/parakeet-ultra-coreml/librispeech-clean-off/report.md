# Teletype Eval Report

- **Git SHA:** 51241ad
- **Timestamp:** 1792262769
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
| Micro WER | 0.0184 |
| Macro WER | 0.0224 |
| Mean polish score | 79.7 |
| ASR p50 / p95 (ms) | 48 / 114 |
| Total p50 / p95 (ms) | 48 / 114 |
| Model load (ms) | 12501 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 4126 | 76 | 0.0184 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-clean-0174 | 0.3333 | 1 | 2 | 0 | this was at the march election eighteen fifty five | This was at the March election, 1855. |
| librispeech-clean-0031 | 0.2857 | 1 | 1 | 0 | the foul mouthed fellow at the top | The foul-mouthed fellow at the top. |
| librispeech-clean-0035 | 0.2222 | 2 | 0 | 2 | goobers don't grow on the tops of vines but underground on t… | Goobles don't grow on the tops of vines, but on the ground, … |
| librispeech-clean-0047 | 0.2222 | 2 | 0 | 0 | now you'll stay cried van say polly won't you | Now you'll stay, cried Anne. Stay, Polly, won't you? |
| librispeech-clean-0101 | 0.2000 | 1 | 0 | 0 | if i can get patients | If I can get patience, |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 44 |
| no_hallucination | 43 |
| terminal_punctuation | 31 |
| sentence_cased | 15 |
| no_fillers | 7 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 35 | 121 | 56 | 48 | 114 | 118 |
| total | 200 | 35 | 121 | 56 | 48 | 114 | 118 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0161 |
| Verbatim WER (macro) | 0.0184 |
| Entity accuracy | 0.9000 |

