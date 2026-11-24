# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792751924
- **Model:** parakeet-tdt-v3-q4
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0187 |
| Macro WER | 0.0211 |
| Mean polish score | 79.9 |
| ASR p50 / p95 (ms) | 129 / 321 |
| Total p50 / p95 (ms) | 129 / 321 |
| Model load (ms) | 161 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 4126 | 77 | 0.0187 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-clean-0035 | 0.2222 | 2 | 0 | 2 | goobers don't grow on the tops of vines but underground on t… | Goobles don't grow on the tops of vines, but on the ground, … |
| librispeech-clean-0047 | 0.2222 | 2 | 0 | 0 | now you'll stay cried van say polly won't you | Now you'll stay, cried Anne. Stay, Polly, won't you? |
| librispeech-clean-0101 | 0.2000 | 1 | 0 | 0 | if i can get patients | If I can get patience. |
| librispeech-clean-0130 | 0.2000 | 1 | 0 | 0 | i boldly lighted my cheroot | I boldly lighted my chirut, |
| librispeech-clean-0064 | 0.1579 | 1 | 2 | 0 | there is no danger of the modern commentators on the timaeus… | There is no danger of the modern commentators on Timaeus fal… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 43 |
| no_hallucination | 43 |
| terminal_punctuation | 37 |
| sentence_cased | 16 |
| no_fillers | 7 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 65 | 640 | 155 | 129 | 321 | 534 |
| total | 200 | 65 | 640 | 155 | 129 | 321 | 534 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0185 |
| Verbatim WER (macro) | 0.0207 |
| Entity accuracy | 0.9000 |

