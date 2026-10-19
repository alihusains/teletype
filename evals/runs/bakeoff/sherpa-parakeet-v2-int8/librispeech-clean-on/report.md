# Teletype Eval Report

- **Git SHA:** 00f8a76
- **Timestamp:** 1792360514
- **Model:** sherpa-parakeet-v2-int8
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0167 |
| Macro WER | 0.0178 |
| Mean polish score | 80.8 |
| ASR p50 / p95 (ms) | 358 / 1129 |
| Total p50 / p95 (ms) | 358 / 1129 |
| Model load (ms) | 748 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 4126 | 69 | 0.0167 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-clean-0035 | 0.2222 | 2 | 0 | 2 | goobers don't grow on the tops of vines but underground on t… | Googles don't grow on the tops of vines, but on the ground, … |
| librispeech-clean-0101 | 0.2000 | 1 | 0 | 0 | if i can get patients | If I can get patience, |
| librispeech-clean-0130 | 0.2000 | 1 | 0 | 0 | i boldly lighted my cheroot | I boldly lighted my cherute, |
| librispeech-clean-0050 | 0.1429 | 1 | 0 | 0 | i never see lou's scythe over here | I never see Lew's scythe over here. |
| librispeech-clean-0072 | 0.1333 | 1 | 0 | 1 | as used in the speech of everyday life the word carries an u… | As used in the speech of every day life, the word carries an… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 42 |
| no_hallucination | 40 |
| terminal_punctuation | 34 |
| sentence_cased | 18 |
| no_fillers | 7 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 124 | 1651 | 480 | 358 | 1129 | 1393 |
| total | 200 | 124 | 1651 | 480 | 358 | 1129 | 1393 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0166 |
| Verbatim WER (macro) | 0.0175 |
| Entity accuracy | 0.9000 |

