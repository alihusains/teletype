# Teletype Eval Report

- **Git SHA:** d4b433b
- **Timestamp:** 1792229519
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
| Micro WER | 0.0568 |
| Macro WER | 0.0743 |
| Mean polish score | 59.9 |
| ASR p50 / p95 (ms) | 680 / 730 |
| Total p50 / p95 (ms) | 680 / 730 |
| Model load (ms) | 2211 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 3505 | 199 | 0.0568 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-other-0022 | 1.0000 | 4 | 0 | 3 | asked zarathustra vehemently what doest thou here | as a twist of vehemently what do we stop here |
| librispeech-other-0138 | 1.0000 | 3 | 0 | 0 | this frees woman | These freeze women |
| librispeech-other-0086 | 0.6667 | 2 | 0 | 0 | he invaded babylonia | "'He invaded Babylonia.' |
| librispeech-other-0012 | 0.4615 | 6 | 0 | 0 | my dear said mister hale alarmed pray don't go off on that i… | "'My dear,' said Mr. Hell, alarmed, "'pray don't go off on t… |
| librispeech-other-0110 | 0.4000 | 1 | 0 | 1 | the mirabelle was nearing tahiti | The mirror bell was nearing Tahiti. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| terminal_punctuation | 123 |
| sentence_cased | 107 |
| no_content_loss | 84 |
| no_hallucination | 81 |
| no_fillers | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 648 | 803 | 685 | 680 | 730 | 746 |
| total | 200 | 648 | 803 | 685 | 680 | 730 | 746 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0398 |
| Verbatim WER (macro) | 0.0530 |
| Entity accuracy | 0.6667 |

