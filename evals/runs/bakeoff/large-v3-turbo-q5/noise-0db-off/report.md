# Teletype Eval Report

- **Git SHA:** d4b433b
- **Timestamp:** 1792230395
- **Model:** large-v3-turbo-q5
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.1046 |
| Macro WER | 0.1210 |
| Mean polish score | 47.0 |
| ASR p50 / p95 (ms) | 678 / 728 |
| Total p50 / p95 (ms) | 678 / 728 |
| Model load (ms) | 2223 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 172 | 0.1046 |
| snr-0db | 100 | 1644 | 172 | 0.1046 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-0db-0010 | 1.1429 | 5 | 0 | 3 | asked zarathustra vehemently what doest thou here | I have to strive MLP. What do I stop here? |
| noise-0db-0025 | 0.7143 | 4 | 1 | 0 | the vil stuck fast in his throat | The veal sucked past the throat. |
| noise-0db-0066 | 0.6154 | 7 | 0 | 1 | he has tried that road and knows that it is the wrong road | He has cried at home and knows that this is a drama for us. |
| noise-0db-0091 | 0.5882 | 6 | 0 | 4 | gurr glanced round to see if the men were looking and then s… | So I glanced around to see if the men were looking and then … |
| noise-0db-0033 | 0.4286 | 2 | 1 | 0 | but you are a jolly fellow too | But you're a jolly fella too. |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 58 |
| no_hallucination | 58 |
| terminal_punctuation | 16 |
| sentence_cased | 12 |
| no_fillers | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 642 | 801 | 682 | 678 | 728 | 761 |
| total | 100 | 642 | 801 | 682 | 678 | 728 | 761 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0977 |
| Verbatim WER (macro) | 0.1154 |

