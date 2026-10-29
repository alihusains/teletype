# Teletype Eval Report

- **Git SHA:** fd63811
- **Timestamp:** 1793203414
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
| Micro WER | 0.3577 |
| Macro WER | 0.3674 |
| Mean polish score | 29.4 |
| ASR p50 / p95 (ms) | 327 / 901 |
| Total p50 / p95 (ms) | 327 / 901 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| competing-speaker | 200 | 4126 | 1476 | 0.3577 |
| librispeech | 200 | 4126 | 1476 | 0.3577 |
| ssr-0db | 200 | 4126 | 1476 | 0.3577 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| competing-0db-0052 | 1.8000 | 5 | 0 | 4 | the horizon seems extremely distant | I say so. We're stuck to moonlight, said Sancho. |
| competing-0db-0063 | 1.4000 | 14 | 0 | 7 | the influence with the timaeus has exercised upon posterity … | Experimenting with his tail and fins and such enthusiasm lik… |
| competing-0db-0188 | 1.3636 | 10 | 0 | 5 | the peculiar circumstances of the colony are within your exc… | The names still showed up around many a time for the safe ho… |
| competing-0db-0108 | 1.3333 | 3 | 0 | 1 | ocean reigned supreme | You see, foreign countries. |
| competing-0db-0195 | 1.3171 | 40 | 0 | 14 | guided by you how we might stroll towards death our only mus… | Can marry day and night, and I tarried some time in this sol… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 158 |
| no_hallucination | 153 |
| terminal_punctuation | 24 |
| no_fillers | 10 |
| sentence_cased | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 145 | 1022 | 401 | 327 | 901 | 999 |
| total | 200 | 145 | 1022 | 401 | 327 | 901 | 999 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.3515 |
| Verbatim WER (macro) | 0.3622 |
| Entity accuracy | 0.8000 |

