# Teletype Eval Report

- **Git SHA:** 8197681
- **Timestamp:** 1792073847
- **Model:** parakeet-ultra
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 1306 |
| Micro WER | 0.0863 |
| Macro WER | 0.0687 |
| Mean polish score | 60.7 |
| ASR p50 / p95 (ms) | 433 / 782 |
| Total p50 / p95 (ms) | 433 / 782 |
| Model load (ms) | 1711 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| 4468919_trimmed | 1 | 2125 | 293 | 0.1379 |
| 4471586 | 1 | 2961 | 341 | 0.1152 |
| 4474955 | 1 | 1738 | 217 | 0.1249 |
| 4475604 | 1 | 2797 | 251 | 0.0897 |
| 4482968 | 1 | 3412 | 466 | 0.1366 |
| 4483046 | 1 | 2781 | 545 | 0.1960 |
| ami | 100 | 761 | 93 | 0.1222 |
| earnings22-vocab | 6 | 15814 | 2113 | 0.1336 |
| fleurs-de | 50 | 1011 | 68 | 0.0673 |
| fleurs-en | 50 | 1095 | 71 | 0.0648 |
| fleurs-es | 50 | 1196 | 64 | 0.0535 |
| fleurs-fr | 50 | 1237 | 79 | 0.0639 |
| fleurs-it | 50 | 1206 | 36 | 0.0299 |
| fleurs-nl | 50 | 1058 | 98 | 0.0926 |
| fleurs-pl | 50 | 961 | 84 | 0.0874 |
| fleurs-pt | 50 | 1158 | 81 | 0.0699 |
| fleurs-ru | 50 | 973 | 74 | 0.0761 |
| fleurs-uk | 50 | 949 | 71 | 0.0748 |
| librispeech | 400 | 7631 | 209 | 0.0274 |
| noise | 300 | 4932 | 309 | 0.0627 |
| snr-0db | 100 | 1644 | 129 | 0.0785 |
| snr-10db | 100 | 1644 | 83 | 0.0505 |
| snr-5db | 100 | 1644 | 97 | 0.0590 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| ami-0029 | 2.0000 | 1 | 0 | 3 | and maybe | And bearing in mind that |
| ami-0058 | 2.0000 | 1 | 0 | 1 | only | Oh my |
| ami-0009 | 1.0000 | 1 | 0 | 0 | mm-hmm | Mm. |
| ami-0018 | 1.0000 | 1 | 0 | 0 | so | Excellent. |
| ami-0019 | 1.0000 | 0 | 1 | 0 | oops |  |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 567 |
| no_hallucination | 559 |
| terminal_punctuation | 118 |
| no_fillers | 62 |
| sentence_cased | 59 |
| bullets_when_listing | 20 |
| no_markdown_fences | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 1305 | 147 | 21917 | 530 | 433 | 782 | 1010 |
| total | 1305 | 147 | 21917 | 530 | 433 | 782 | 1010 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0789 |
| Verbatim WER (macro) | 0.0620 |
| Vocab recall | 0.8277 |
| Vocab false-alarm rate | 0.0000 |
| Entity accuracy | 0.8097 |

