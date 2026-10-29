# Teletype Eval Report

- **Git SHA:** fd63811
- **Timestamp:** 1793203110
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
| Micro WER | 0.0722 |
| Macro WER | 0.0799 |
| Mean polish score | 56.6 |
| ASR p50 / p95 (ms) | 294 / 670 |
| Total p50 / p95 (ms) | 294 / 670 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 3505 | 253 | 0.0722 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-other-0022 | 1.2857 | 5 | 0 | 4 | asked zarathustra vehemently what doest thou here | I have to straight away immediately. What do I stop here? |
| librispeech-other-0162 | 0.5000 | 1 | 0 | 1 | is platterbaff out yet | Is pleather bath out yet? |
| librispeech-other-0110 | 0.4000 | 1 | 0 | 1 | the mirabelle was nearing tahiti | The mirror bell was nearing Tahiti. |
| librispeech-other-0016 | 0.3750 | 4 | 2 | 0 | if e's straight e'll do for me and if he ain't i'll do for i… | If he's straight, he'll do for me, and fiend, I'll do for hi… |
| librispeech-other-0035 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | What are you bound for, young man? |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 97 |
| no_hallucination | 95 |
| terminal_punctuation | 6 |
| no_fillers | 4 |
| sentence_cased | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 127 | 1128 | 339 | 294 | 670 | 799 |
| total | 200 | 127 | 1128 | 339 | 294 | 670 | 799 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0638 |
| Verbatim WER (macro) | 0.0715 |
| Entity accuracy | 1.0000 |

