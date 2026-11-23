# Teletype Eval Report

- **Git SHA:** cee77e6
- **Timestamp:** 1792609534
- **Model:** parakeet-ultra-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0462 |
| Macro WER | 0.0546 |
| Mean polish score | 66.6 |
| ASR p50 / p95 (ms) | 44 / 80 |
| Total p50 / p95 (ms) | 44 / 80 |
| Model load (ms) | 95 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 76 | 0.0462 |
| snr-10db | 100 | 1644 | 76 | 0.0462 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-10db-0010 | 0.4286 | 2 | 0 | 1 | asked zarathustra vehemently what doest thou here | asked their Twistra vehemently, What doest thou hear? |
| noise-10db-0019 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | Where are you bound for, young man? |
| noise-10db-0037 | 0.3125 | 5 | 0 | 0 | will yo' alls please speak a lil louder he holler down the c… | Will you all please speak a little louder? he hollered down … |
| noise-10db-0029 | 0.2857 | 2 | 0 | 0 | die thou fafnir and then fafnir died | Die thou Fafner. And then Fafner died. |
| noise-10db-0033 | 0.2857 | 1 | 1 | 0 | but you are a jolly fellow too | But you're a jolly fellow too. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_hallucination | 37 |
| no_content_loss | 36 |
| sentence_cased | 12 |
| terminal_punctuation | 9 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 33 | 119 | 48 | 44 | 80 | 110 |
| total | 100 | 33 | 119 | 48 | 44 | 80 | 110 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0434 |
| Verbatim WER (macro) | 0.0509 |

