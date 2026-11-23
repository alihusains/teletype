# Teletype Eval Report

- **Git SHA:** d4b433b
- **Timestamp:** 1792224416
- **Model:** parakeet-tdt-v3
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.0505 |
| Macro WER | 0.0596 |
| Mean polish score | 63.9 |
| ASR p50 / p95 (ms) | 157 / 259 |
| Total p50 / p95 (ms) | 157 / 259 |
| Model load (ms) | 212 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 83 | 0.0505 |
| snr-10db | 100 | 1644 | 83 | 0.0505 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-10db-0010 | 0.4286 | 3 | 0 | 0 | asked zarathustra vehemently what doest thou here | Azir twistra vehemently, What doest thou hear? |
| noise-10db-0019 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | Where are you bound for, young man? |
| noise-10db-0037 | 0.3125 | 4 | 1 | 0 | will yo' alls please speak a lil louder he holler down the c… | Will y'all please speak a little louder? he hollered down th… |
| noise-10db-0029 | 0.2857 | 2 | 0 | 0 | die thou fafnir and then fafnir died | Die thou Fafner. And then Fafner died. |
| noise-10db-0033 | 0.2857 | 1 | 1 | 0 | but you are a jolly fellow too | But you're a jolly fellow too. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_hallucination | 40 |
| no_content_loss | 39 |
| terminal_punctuation | 14 |
| sentence_cased | 8 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 84 | 395 | 172 | 157 | 259 | 330 |
| total | 100 | 84 | 395 | 172 | 157 | 259 | 330 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0476 |
| Verbatim WER (macro) | 0.0560 |

