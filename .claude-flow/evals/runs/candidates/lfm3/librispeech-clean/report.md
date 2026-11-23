# Teletype Eval Report

- **Git SHA:** fd63811
- **Timestamp:** 1793203040
- **Model:** lfm3
- **Mode:** replay

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0257 |
| Macro WER | 0.0309 |
| Mean polish score | 75.8 |
| ASR p50 / p95 (ms) | 304 / 771 |
| Total p50 / p95 (ms) | 304 / 771 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 4126 | 106 | 0.0257 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-clean-0174 | 0.3333 | 1 | 2 | 0 | this was at the march election eighteen fifty five | This was at the March election, 1855. |
| librispeech-clean-0031 | 0.2857 | 1 | 1 | 0 | the foul mouthed fellow at the top | The foul-mouthed fellow at the top. |
| librispeech-clean-0173 | 0.2857 | 1 | 1 | 0 | the bogus legislature numbered thirty six members | The bogus legislature numbered 36 members. |
| librispeech-clean-0035 | 0.2222 | 2 | 0 | 2 | goobers don't grow on the tops of vines but underground on t… | Goubles don't grow on the tops of vines, but on the ground, … |
| librispeech-clean-0067 | 0.2143 | 2 | 0 | 1 | tell us said the other the whole story and where solon heard… | Tell us, said the other, the whole story, and where Saul and… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 54 |
| no_hallucination | 53 |
| no_fillers | 7 |
| terminal_punctuation | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 136 | 1132 | 373 | 304 | 771 | 930 |
| total | 200 | 136 | 1132 | 373 | 304 | 771 | 930 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0195 |
| Verbatim WER (macro) | 0.0223 |
| Entity accuracy | 0.9000 |

