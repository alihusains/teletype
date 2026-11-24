# Teletype Eval Report

- **Git SHA:** fd63811
- **Timestamp:** 1793203470
- **Model:** lfm3
- **Mode:** replay

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 50 |
| Micro WER | 0.1103 |
| Macro WER | 0.1296 |
| Mean polish score | 32.6 |
| ASR p50 / p95 (ms) | 577 / 1046 |
| Total p50 / p95 (ms) | 577 / 1046 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| fleurs-pl | 50 | 961 | 106 | 0.1103 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| fleurs-pl-0044 | 0.6667 | 7 | 1 | 0 | lakkha singh przedstawił też chhappan bhog bhadżan akompanio… | Lak Kasingh, przedstawił też Chapan Bokbajan, akompaniewał m… |
| fleurs-pl-0031 | 0.6000 | 2 | 0 | 1 | także zapaśnicy złożyli hołd lunie | Także za paśnice złożyli hold lunie. |
| fleurs-pl-0021 | 0.3750 | 2 | 1 | 0 | paryżan uważa się za egocentrycznych grubiańskich i zadufany… | Paryżan uważa się za egocentrycznych, grubińskich, renufowan… |
| fleurs-pl-0039 | 0.3333 | 5 | 1 | 0 | sundarbany wpisano na listę światowego dziedzictwa unesco cz… | Sundarbany wpisana na listę światowego dziedzictwa UNESCO. R… |
| fleurs-pl-0024 | 0.3182 | 5 | 0 | 2 | kongres rozpoczął finansowanie inicjatywy dotyczącej obsceni… | Konkretny plan sposobu finansowania inicjatywy dotyczącej ob… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_hallucination | 38 |
| no_content_loss | 37 |
| bullets_when_listing | 1 |
| terminal_punctuation | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 50 | 255 | 1162 | 602 | 577 | 1046 | 1162 |
| total | 50 | 255 | 1162 | 602 | 577 | 1046 | 1162 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.1132 |
| Verbatim WER (macro) | 0.1295 |
| Entity accuracy | 0.6364 |

