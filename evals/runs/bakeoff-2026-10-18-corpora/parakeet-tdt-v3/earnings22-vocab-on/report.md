# Teletype Eval Report

- **Git SHA:** bea3992
- **Timestamp:** 1792287741
- **Model:** parakeet-tdt-v3
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 6 |
| Micro WER | 0.1336 |
| Macro WER | 0.1333 |
| Mean polish score | 6.0 |
| ASR p50 / p95 (ms) | 12749 / 19983 |
| Total p50 / p95 (ms) | 12749 / 19983 |
| Model load (ms) | 225 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| 4468919_trimmed | 1 | 2125 | 293 | 0.1379 |
| 4471586 | 1 | 2961 | 340 | 0.1148 |
| 4474955 | 1 | 1738 | 217 | 0.1249 |
| 4475604 | 1 | 2797 | 251 | 0.0897 |
| 4482968 | 1 | 3412 | 466 | 0.1366 |
| 4483046 | 1 | 2781 | 545 | 0.1960 |
| earnings22-vocab | 6 | 15814 | 2112 | 0.1336 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| earnings22-vocab-0006 | 0.1960 | 148 | 369 | 28 | now it's on times. they would like to starts the performance… | Today's speaker is the Executive Vice President Tizuji Yaman… |
| earnings22-vocab-0001 | 0.1379 | 97 | 185 | 11 | good morning and welcome to the loma negra third quarter 202… | Good morning and welcome to the Loma Negra third quarter 202… |
| earnings22-vocab-0005 | 0.1366 | 174 | 266 | 26 | thank you, darcy, and welcome everyone to our december quart… | Thank you, Darcy, and welcome everyone to our December quart… |
| earnings22-vocab-0003 | 0.1249 | 102 | 92 | 23 | hello ladies and gentlemen, thank you for standing by for qu… | Hello, ladies and gentlemen. Thank you for standing by for C… |
| earnings22-vocab-0002 | 0.1148 | 141 | 167 | 32 | please stand by, we're about to begin. good day and welcome … | Mm-hmm by we're about to begin. Good day, and welcome to the… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| bullets_when_listing | 6 |
| no_content_loss | 6 |
| no_fillers | 6 |
| no_hallucination | 6 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 6 | 10090 | 19983 | 13675 | 12749 | 19983 | 19983 |
| total | 6 | 10090 | 19983 | 13675 | 12749 | 19983 | 19983 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1218 |
| Verbatim WER (macro) | 0.1227 |
| Vocab recall | 0.8277 |
| Vocab false-alarm rate | 0.0000 |
| Entity accuracy | 0.8447 |

