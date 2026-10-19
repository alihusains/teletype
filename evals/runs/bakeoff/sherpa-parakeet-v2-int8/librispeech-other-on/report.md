# Teletype Eval Report

- **Git SHA:** 00f8a76
- **Timestamp:** 1792360701
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
| Micro WER | 0.0382 |
| Macro WER | 0.0446 |
| Mean polish score | 66.2 |
| ASR p50 / p95 (ms) | 325 / 952 |
| Total p50 / p95 (ms) | 325 / 952 |
| Model load (ms) | 637 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 3505 | 134 | 0.0382 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-other-0035 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | Where are you bound for, young man? |
| librispeech-other-0138 | 0.3333 | 1 | 0 | 0 | this frees woman | This frees women. |
| librispeech-other-0080 | 0.3125 | 5 | 0 | 0 | will yo' alls please speak a lil louder he holler down the c… | Will you all please speak a little louder? he hollered down … |
| librispeech-other-0022 | 0.2857 | 2 | 0 | 0 | asked zarathustra vehemently what doest thou here | answered Tustra vehemently, what doest thou here? |
| librispeech-other-0062 | 0.2857 | 2 | 0 | 0 | die thou fafnir and then fafnir died | Die, thou, Fafner And then Fafner died. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 75 |
| no_hallucination | 72 |
| terminal_punctuation | 28 |
| sentence_cased | 19 |
| no_fillers | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 124 | 1677 | 423 | 325 | 952 | 1282 |
| total | 200 | 124 | 1677 | 423 | 325 | 952 | 1282 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0342 |
| Verbatim WER (macro) | 0.0413 |
| Entity accuracy | 1.0000 |

