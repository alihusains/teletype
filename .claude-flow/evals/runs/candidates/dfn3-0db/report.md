# Teletype Eval Report

- **Git SHA:** 4361547
- **Timestamp:** 1793040508
- **Model:** dfn3+fluid-mini
- **Mode:** replay

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.5405 |
| Macro WER | 0.5533 |
| Mean polish score | 21.1 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| competing-speaker | 200 | 4126 | 2230 | 0.5405 |
| librispeech | 200 | 4126 | 2230 | 0.5405 |
| ssr-0db | 200 | 4126 | 2230 | 0.5405 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| competing-0db-0188 | 1.6364 | 9 | 0 | 9 | the peculiar circumstances of the colony are within your exc… | But the name is still shown to Sir Gross many a time over th… |
| competing-0db-0032 | 1.5000 | 3 | 0 | 3 | no sir certainly not | The whole similar system is certainly enjoyment. |
| competing-0db-0063 | 1.3333 | 15 | 0 | 5 | the influence with the timaeus has exercised upon posterity … | Pyramiding continued, as we're sometimes likely, some people… |
| competing-0db-0137 | 1.3333 | 9 | 0 | 3 | nay nay lording answered warrenton with a half laugh | Of which he made a compound, mixing them all and boiling the… |
| competing-0db-0195 | 1.3171 | 40 | 0 | 14 | guided by you how we might stroll towards death our only mus… | in merry day and night, and I tarried some time in their sol… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 181 |
| no_hallucination | 162 |
| terminal_punctuation | 68 |
| sentence_cased | 28 |
| no_fillers | 8 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.5403 |
| Verbatim WER (macro) | 0.5507 |
| Entity accuracy | 0.2000 |

