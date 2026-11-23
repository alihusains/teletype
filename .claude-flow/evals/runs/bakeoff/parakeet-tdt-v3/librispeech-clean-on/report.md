# Teletype Eval Report

- **Git SHA:** d4b433b
- **Timestamp:** 1792223987
- **Model:** parakeet-tdt-v3
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0170 |
| Macro WER | 0.0205 |
| Mean polish score | 82.0 |
| ASR p50 / p95 (ms) | 199 / 360 |
| Total p50 / p95 (ms) | 199 / 360 |
| Model load (ms) | 317 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 4126 | 70 | 0.0170 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-clean-0035 | 0.2222 | 2 | 0 | 2 | goobers don't grow on the tops of vines but underground on t… | Goobles don't grow on the tops of vines, but on the ground, … |
| librispeech-clean-0047 | 0.2222 | 2 | 0 | 0 | now you'll stay cried van say polly won't you | Now you'll stay, cried Anne. Stay, Polly, won't you? |
| librispeech-clean-0101 | 0.2000 | 1 | 0 | 0 | if i can get patients | If I can get patience |
| librispeech-clean-0130 | 0.2000 | 1 | 0 | 0 | i boldly lighted my cheroot | I boldly lighted my chirut, |
| librispeech-clean-0147 | 0.1875 | 2 | 0 | 1 | i was bookkeeper so it was easy to get a blank check and for… | I was bit keeper, so it was easy to get a blank cheque and f… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| terminal_punctuation | 41 |
| no_hallucination | 39 |
| no_content_loss | 38 |
| sentence_cased | 13 |
| no_fillers | 7 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 102 | 576 | 222 | 199 | 360 | 535 |
| total | 200 | 102 | 576 | 222 | 199 | 360 | 535 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0168 |
| Verbatim WER (macro) | 0.0202 |
| Entity accuracy | 0.9000 |

