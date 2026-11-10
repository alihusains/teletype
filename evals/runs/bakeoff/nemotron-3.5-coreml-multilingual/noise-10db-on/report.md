# Teletype Eval Report

- **Git SHA:** 79524a3
- **Timestamp:** 1794279620
- **Model:** nemotron-3.5-coreml-multilingual
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 100 |
| Micro WER | 0.1509 |
| Macro WER | 0.1835 |
| Mean polish score | 45.6 |
| ASR p50 / p95 (ms) | 97 / 309 |
| Total p50 / p95 (ms) | 97 / 309 |
| Model load (ms) | 134 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| noise | 100 | 1644 | 248 | 0.1509 |
| snr-10db | 100 | 1644 | 248 | 0.1509 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| noise-10db-0012 | 1.0000 | 6 | 9 | 0 | for it do i ask and seek and have sought but have not found … | फाइट टू इश नॉट फॉर्म इट |
| noise-10db-0026 | 1.0000 | 7 | 2 | 0 | general observations on preserves confectionary ices and des… | जनरल ऑब्जर्वेशन्स ऑन पर्सनरी, इस, एंड डिश |
| noise-10db-0032 | 1.0000 | 5 | 0 | 0 | asked jean valjean fauchelevent replied | Students argent, first Levon repli |
| noise-10db-0076 | 1.0000 | 4 | 7 | 0 | perhaps she could speak for them more than for some others | Ще худ с пек |
| noise-10db-0087 | 1.0000 | 4 | 1 | 0 | not you only a sham | Na ju, unija š |

## Polish Failed Checks

| Check | Failures |
|---|---|
| terminal_punctuation | 98 |
| no_content_loss | 59 |
| no_hallucination | 57 |
| no_fillers | 2 |
| sentence_cased | 2 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 100 | 44 | 583 | 129 | 97 | 309 | 558 |
| total | 100 | 44 | 583 | 129 | 97 | 309 | 558 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1508 |
| Verbatim WER (macro) | 0.1834 |

