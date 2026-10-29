# Teletype Eval Report

- **Git SHA:** fd63811
- **Timestamp:** 1793203550
- **Model:** large-v3-turbo-q5
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 50 |
| Micro WER | 1.1686 |
| Macro WER | 1.1670 |
| Mean polish score | 11.5 |
| ASR p50 / p95 (ms) | 767 / 1225 |
| Total p50 / p95 (ms) | 767 / 1225 |
| Model load (ms) | 2496 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| fleurs-pl | 50 | 961 | 1123 | 1.1686 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| fleurs-pl-0041 | 2.0645 | 29 | 0 | 35 | wśród tematów omawianych na bali znalazł się również problem… | In the area of the area, the problem of the las, which remai… |
| fleurs-pl-0010 | 1.7143 | 21 | 0 | 15 | wyłącznie mutacje w komórkach linii zarodkowej mogą być prze… | Only mutations in the lines of the lines can be sent to the … |
| fleurs-pl-0042 | 1.6190 | 21 | 0 | 13 | zadaniem marynarki wojennej od zawsze było zapewnienie krajo… | The mission of the Marnarka War was always to be given the c… |
| fleurs-pl-0031 | 1.6000 | 5 | 0 | 3 | także zapaśnicy złożyli hołd lunie | Also the soldiers had to be a holloon. |
| fleurs-pl-0016 | 1.4762 | 19 | 0 | 12 | 53-letni cuomo objął na początku tego roku stanowisko gubern… | 53-year-old Cuomo was in the beginning of this year's positi… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 49 |
| no_hallucination | 49 |
| sentence_cased | 2 |
| terminal_punctuation | 2 |
| bullets_when_listing | 1 |
| no_fillers | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 50 | 678 | 1549 | 805 | 767 | 1225 | 1549 |
| total | 50 | 678 | 1549 | 805 | 767 | 1225 | 1549 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 1.1755 |
| Verbatim WER (macro) | 1.1721 |
| Entity accuracy | 0.5455 |

