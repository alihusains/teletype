# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792751906
- **Model:** parakeet-v3-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0523 |
| Macro WER | 0.0697 |
| Mean polish score | 63.6 |
| ASR p50 / p95 (ms) | 41 / 79 |
| Total p50 / p95 (ms) | 41 / 79 |
| Model load (ms) | 93 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 86 | 0.0523 |
| snr-10db | 100 | 1644 | 86 | 0.0523 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-10db-0008 | 1.0000 | 0 | 0 | 3 | and also these | And also these are the two. |
| noise-10db-0010 | 0.4286 | 3 | 0 | 0 | asked zarathustra vehemently what doest thou here | Azir twistra vehemently, What doest thou hear? |
| noise-10db-0019 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | Where are you bound for, young man? |
| noise-10db-0037 | 0.3125 | 4 | 1 | 0 | will yo' alls please speak a lil louder he holler down the c… | Will y'all please speak a little louder? he hollered down th… |
| noise-10db-0029 | 0.2857 | 2 | 0 | 0 | die thou fafnir and then fafnir died | Die thou Fafner. And then Fafner died. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_hallucination | 41 |
| no_content_loss | 39 |
| sentence_cased | 9 |
| terminal_punctuation | 7 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 33 | 112 | 47 | 41 | 79 | 109 |
| total | 100 | 33 | 112 | 47 | 41 | 79 | 109 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0495 |
| Verbatim WER (macro) | 0.0660 |

