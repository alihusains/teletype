# Teletype Eval Report

- **Git SHA:** fd63811
- **Timestamp:** 1793203331
- **Model:** lfm3
- **Mode:** replay

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.1326 |
| Macro WER | 0.1504 |
| Mean polish score | 44.7 |
| ASR p50 / p95 (ms) | 292 / 670 |
| Total p50 / p95 (ms) | 292 / 670 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 218 | 0.1326 |
| snr-0db | 100 | 1644 | 218 | 0.1326 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-0db-0010 | 1.2857 | 5 | 0 | 4 | asked zarathustra vehemently what doest thou here | I just wish to have HP. What do I have here? |
| noise-0db-0033 | 0.8571 | 3 | 3 | 0 | but you are a jolly fellow too | But you're enjoying, fellas. |
| noise-0db-0025 | 0.7143 | 5 | 0 | 0 | the vil stuck fast in his throat | The veal sucked fast to the circle. |
| noise-0db-0091 | 0.5882 | 6 | 0 | 4 | gurr glanced round to see if the men were looking and then s… | I glanced around to see if the men were looking, and then sa… |
| noise-0db-0029 | 0.5714 | 4 | 0 | 0 | die thou fafnir and then fafnir died | God about Fathom, and then Fathom died. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 63 |
| no_hallucination | 59 |
| terminal_punctuation | 4 |
| no_fillers | 2 |
| sentence_cased | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 142 | 1197 | 357 | 292 | 670 | 945 |
| total | 100 | 142 | 1197 | 357 | 292 | 670 | 945 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1279 |
| Verbatim WER (macro) | 0.1453 |

