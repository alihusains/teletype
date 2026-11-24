# Teletype Eval Report

- **Git SHA:** 19b2767
- **Timestamp:** 1793033795
- **Model:** fluid-speech-mini
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0162 |
| Macro WER | 0.0188 |
| Mean polish score | 81.7 |
| ASR p50 / p95 (ms) | 156 / 437 |
| Total p50 / p95 (ms) | 156 / 437 |
| Model load (ms) | 987 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 4126 | 67 | 0.0162 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-clean-0035 | 0.2222 | 2 | 0 | 2 | goobers don't grow on the tops of vines but underground on t… | Goobles don't grow on the tops of vines, but on the ground, … |
| librispeech-clean-0047 | 0.2222 | 2 | 0 | 0 | now you'll stay cried van say polly won't you | Now you'll stay, cried Anne. Stay, Polly, won't you? |
| librispeech-clean-0101 | 0.2000 | 1 | 0 | 0 | if i can get patients | If I can get patience, |
| librispeech-clean-0130 | 0.2000 | 1 | 0 | 0 | i boldly lighted my cheroot | I boldly lighted my chirut, |
| librispeech-clean-0150 | 0.1667 | 3 | 0 | 1 | the weak kneed contingency must be strengthened and fortifie… | the weak need contingency must be strengthened and fortified… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| terminal_punctuation | 42 |
| no_content_loss | 39 |
| no_hallucination | 39 |
| sentence_cased | 15 |
| no_fillers | 7 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 78 | 702 | 195 | 156 | 437 | 490 |
| total | 200 | 78 | 702 | 195 | 156 | 437 | 490 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0159 |
| Verbatim WER (macro) | 0.0182 |
| Entity accuracy | 0.9000 |

