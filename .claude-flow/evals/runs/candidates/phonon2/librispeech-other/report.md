# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792775364
- **Model:** phonon2
- **Mode:** replay

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0525 |
| Macro WER | 0.0598 |
| Mean polish score | 62.8 |
| ASR p50 / p95 (ms) | 19 / 121 |
| Total p50 / p95 (ms) | 19 / 121 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 3505 | 184 | 0.0525 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-other-0022 | 0.5714 | 3 | 0 | 1 | asked zarathustra vehemently what doest thou here | as the Twistra vehemently, What doest thou hear? |
| librispeech-other-0162 | 0.5000 | 1 | 0 | 1 | is platterbaff out yet | Is Flatter Bath out yet? |
| librispeech-other-0173 | 0.4000 | 0 | 0 | 2 | no wait another half hour | No, wait another and a half hour. |
| librispeech-other-0035 | 0.3333 | 1 | 0 | 1 | wher you bound for young man | Where are you bound for, young man? |
| librispeech-other-0138 | 0.3333 | 1 | 0 | 0 | this frees woman | This frees women. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_hallucination | 82 |
| no_content_loss | 81 |
| terminal_punctuation | 19 |
| sentence_cased | 17 |
| no_fillers | 4 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 8 | 85911 | 1564 | 19 | 121 | 81864 |
| total | 200 | 8 | 85911 | 1564 | 19 | 121 | 81864 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0519 |
| Verbatim WER (macro) | 0.0589 |
| Entity accuracy | 1.0000 |

