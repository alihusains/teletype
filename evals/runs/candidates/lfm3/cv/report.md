# Teletype Eval Report

- **Git SHA:** fd63811
- **Timestamp:** 1793203270
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
| Micro WER | 0.1372 |
| Macro WER | 0.2858 |
| Mean polish score | 40.1 |
| ASR p50 / p95 (ms) | 457 / 2041 |
| Total p50 / p95 (ms) | 457 / 2041 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| cv | 200 | 7349 | 1008 | 0.1372 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| cv-0145 | 13.0000 | 1 | 0 | 12 | [silence] | the first thing we need to do is to look at the data. |
| cv-0150 | 13.0000 | 1 | 0 | 12 | [silence] | the first thing we need to do is to look at the data. |
| cv-0139 | 6.0000 | 1 | 0 | 5 | [silence] | well, it's not a big deal. |
| cv-0118 | 1.0000 | 3 | 0 | 1 | my earliest memories was... | My Alice memory eats us. |
| cv-0117 | 0.6667 | 2 | 0 | 0 | yes  hear that | Yes, she does. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 136 |
| no_hallucination | 122 |
| no_fillers | 70 |
| bullets_when_listing | 4 |
| sentence_cased | 3 |
| terminal_punctuation | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 167 | 3814 | 785 | 457 | 2041 | 3091 |
| total | 200 | 167 | 3814 | 785 | 457 | 2041 | 3091 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1048 |
| Verbatim WER (macro) | 0.0997 |
| Entity accuracy | 0.8235 |

