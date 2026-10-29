# Teletype Eval Report

- **Git SHA:** fd63811
- **Timestamp:** 1793203525
- **Model:** parakeet-ultra-coreml
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 50 |
| Micro WER | 0.0801 |
| Macro WER | 0.0878 |
| Mean polish score | 42.2 |
| ASR p50 / p95 (ms) | 75 / 185 |
| Total p50 / p95 (ms) | 75 / 185 |
| Model load (ms) | 13812 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| fleurs-pl | 50 | 961 | 77 | 0.0801 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| fleurs-pl-0044 | 0.7500 | 8 | 0 | 1 | lakkha singh przedstawił też chhappan bhog bhadżan akompanio… | Luc Hassing przedstawił też Chapan Bokba John, a komponiował… |
| fleurs-pl-0012 | 0.2917 | 7 | 0 | 0 | australijczyk mitchell gourley zajął jedenaste miejsce w sup… | Australijczyk Michael Gurley zajęł 11 miejsce w Supergigenci… |
| fleurs-pl-0002 | 0.2381 | 5 | 0 | 0 | nextgen jest systemem który wg faa umożliwiłby samolotom lot… | Nęczchen jest systemem, który według FAA umożliwił bistamolo… |
| fleurs-pl-0017 | 0.2222 | 2 | 0 | 2 | wielka piramida zbudowana w iii w p.n.e. jest jedną z wielu … | Wielka piramida zbudowana w III wieku przed naszą erą, jest … |
| fleurs-pl-0038 | 0.1905 | 3 | 1 | 0 | haitański instytut sprawiedliwości i demokracji odniósł się … | Haitański Instytut Sprawiedliwości i Demokracji odniósł sied… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 32 |
| no_hallucination | 32 |
| sentence_cased | 2 |
| bullets_when_listing | 1 |
| terminal_punctuation | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 50 | 49 | 197 | 82 | 75 | 185 | 197 |
| total | 50 | 49 | 197 | 82 | 75 | 185 | 197 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0779 |
| Verbatim WER (macro) | 0.0854 |
| Entity accuracy | 0.9091 |

