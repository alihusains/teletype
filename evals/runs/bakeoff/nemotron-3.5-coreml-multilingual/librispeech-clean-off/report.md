# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794278797
- **Model:** nemotron-3.5-coreml-multilingual
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0286 |
| Macro WER | 0.0295 |
| Mean polish score | 69.2 |
| ASR p50 / p95 (ms) | 105 / 325 |
| Total p50 / p95 (ms) | 105 / 325 |
| Model load (ms) | 308708 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| librispeech | 200 | 4126 | 118 | 0.0286 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| librispeech-clean-0199 | 0.3000 | 2 | 1 | 0 | why an ear a whirlpool fierce to draw creations in | Wyan ear, a whirlpool fierced to draw creations in |
| librispeech-clean-0101 | 0.2000 | 1 | 0 | 0 | if i can get patients | If I can get patience |
| librispeech-clean-0130 | 0.2000 | 1 | 0 | 0 | i boldly lighted my cheroot | I boldly lighted my Charout |
| librispeech-clean-0113 | 0.1905 | 3 | 0 | 1 | and with it i leave you a name sif the friendly i shall hope… | And with it I leave you a name siff the friendly I shall hop… |
| librispeech-clean-0048 | 0.1818 | 1 | 0 | 1 | oh no jasper i must go by my very own self | Oh no, Jap sir, I must go by my very own self |

## Polish Failed Checks

| Check | Failures |
|---|---|
| terminal_punctuation | 191 |
| no_content_loss | 64 |
| no_hallucination | 64 |
| no_fillers | 7 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 42 | 716 | 139 | 105 | 325 | 617 |
| total | 200 | 42 | 716 | 139 | 105 | 325 | 617 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0286 |
| Verbatim WER (macro) | 0.0301 |
| Entity accuracy | 0.9000 |

