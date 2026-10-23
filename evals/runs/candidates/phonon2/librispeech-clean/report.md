# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792775364
- **Model:** phonon2
- **Mode:** replay

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0172 |
| Macro WER | 0.0207 |
| Mean polish score | 79.4 |
| ASR p50 / p95 (ms) | 20 / 125 |
| Total p50 / p95 (ms) | 20 / 125 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 4126 | 71 | 0.0172 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-clean-0035 | 0.2222 | 2 | 0 | 2 | goobers don't grow on the tops of vines but underground on t… | Goobles don't grow on the tops of vines, but on the ground, … |
| librispeech-clean-0101 | 0.2000 | 1 | 0 | 0 | if i can get patients | If I can get patience |
| librispeech-clean-0130 | 0.2000 | 1 | 0 | 0 | i boldly lighted my cheroot | I boldly lighted my chirout, |
| librispeech-clean-0055 | 0.1429 | 1 | 0 | 0 | truly this sea is of infinite width | Truly the sea is of infinite width, |
| librispeech-clean-0067 | 0.1429 | 2 | 0 | 0 | tell us said the other the whole story and where solon heard… | Tell us, said the other, the whole story, and where Solin he… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 45 |
| no_hallucination | 44 |
| terminal_punctuation | 29 |
| sentence_cased | 11 |
| no_fillers | 7 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 9 | 381 | 36 | 20 | 125 | 252 |
| total | 200 | 9 | 381 | 36 | 20 | 125 | 252 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0168 |
| Verbatim WER (macro) | 0.0203 |
| Entity accuracy | 0.9000 |

