# Teletype Eval Report

- **Git SHA:** cee77e6
- **Timestamp:** 1792632676
- **Model:** large-v3-turbo-q5
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0356 |
| Macro WER | 0.0481 |
| Mean polish score | 69.9 |
| ASR p50 / p95 (ms) | 686 / 768 |
| Total p50 / p95 (ms) | 686 / 768 |
| Model load (ms) | 245 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 4126 | 147 | 0.0356 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-clean-0047 | 0.5556 | 5 | 0 | 0 | now you'll stay cried van say polly won't you | "'Now you'll stay,' cried Anne. "'Say, Polly, won't you?' |
| librispeech-clean-0032 | 0.5000 | 2 | 0 | 0 | no sir certainly not | "'No, sir, certainly not.' |
| librispeech-clean-0090 | 0.4000 | 2 | 0 | 0 | i must know about you | "'I must know about you.' |
| librispeech-clean-0035 | 0.3333 | 3 | 1 | 2 | goobers don't grow on the tops of vines but underground on t… | Goobas don't grow on the topsy vines, but on the ground, on … |
| librispeech-clean-0174 | 0.3333 | 1 | 2 | 0 | this was at the march election eighteen fifty five | This was at the March election, 1855. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| terminal_punctuation | 110 |
| sentence_cased | 95 |
| no_content_loss | 61 |
| no_hallucination | 61 |
| no_fillers | 7 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 640 | 1061 | 698 | 686 | 768 | 819 |
| total | 200 | 640 | 1061 | 698 | 686 | 768 | 819 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0176 |
| Verbatim WER (macro) | 0.0186 |
| Entity accuracy | 1.0000 |

