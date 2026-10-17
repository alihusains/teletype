# Teletype Eval Report

- **Git SHA:** 0e3ccb2
- **Timestamp:** 1792269793
- **Model:** sherpa-parakeet-v3-int8
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0179 |
| Macro WER | 0.0215 |
| Mean polish score | 79.9 |
| ASR p50 / p95 (ms) | 547 / 1976 |
| Total p50 / p95 (ms) | 547 / 1976 |
| Model load (ms) | 1072 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 4126 | 74 | 0.0179 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-clean-0122 | 0.3333 | 0 | 3 | 0 | effects of the increased use and disuse of parts | Effects of the increased of parts. |
| librispeech-clean-0035 | 0.2222 | 2 | 0 | 2 | goobers don't grow on the tops of vines but underground on t… | Goobles don't grow on the tops of vines, but on the ground, … |
| librispeech-clean-0101 | 0.2000 | 1 | 0 | 0 | if i can get patients | If I can get patience |
| librispeech-clean-0130 | 0.2000 | 1 | 0 | 0 | i boldly lighted my cheroot | I boldly lighted my chirut, |
| librispeech-clean-0031 | 0.1429 | 1 | 0 | 0 | the foul mouthed fellow at the top | The foul mouth fellow at the top. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 43 |
| no_hallucination | 43 |
| terminal_punctuation | 31 |
| sentence_cased | 23 |
| no_fillers | 7 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 177 | 13289 | 909 | 547 | 1976 | 5940 |
| total | 200 | 177 | 13289 | 909 | 547 | 1976 | 5940 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0183 |
| Verbatim WER (macro) | 0.0217 |
| Entity accuracy | 0.9000 |

