# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792751835
- **Model:** parakeet-v3-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0177 |
| Macro WER | 0.0212 |
| Mean polish score | 81.0 |
| ASR p50 / p95 (ms) | 45 / 113 |
| Total p50 / p95 (ms) | 45 / 113 |
| Model load (ms) | 26376 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 4126 | 73 | 0.0177 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-clean-0035 | 0.2222 | 2 | 0 | 2 | goobers don't grow on the tops of vines but underground on t… | Goobles don't grow on the tops of vines, but on the ground, … |
| librispeech-clean-0101 | 0.2000 | 1 | 0 | 0 | if i can get patients | If I can get patience |
| librispeech-clean-0130 | 0.2000 | 1 | 0 | 0 | i boldly lighted my cheroot | I boldly lighted my chirut, |
| librispeech-clean-0147 | 0.1875 | 2 | 0 | 1 | i was bookkeeper so it was easy to get a blank check and for… | I was bit keeper, so it was easy to get a blank cheque and f… |
| librispeech-clean-0026 | 0.1429 | 1 | 0 | 1 | the man entered and took the papers sheet by sheet from the … | The men entered and took the papers, sheet by sheet from the… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_hallucination | 41 |
| terminal_punctuation | 41 |
| no_content_loss | 40 |
| sentence_cased | 14 |
| no_fillers | 7 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 34 | 119 | 53 | 45 | 113 | 117 |
| total | 200 | 34 | 119 | 53 | 45 | 113 | 117 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0173 |
| Verbatim WER (macro) | 0.0209 |
| Entity accuracy | 0.9000 |

