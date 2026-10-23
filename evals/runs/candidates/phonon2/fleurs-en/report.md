# Teletype Eval Report

- **Git SHA:** cbc6b1d
- **Timestamp:** 1792775464
- **Model:** phonon2
- **Mode:** replay

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 50 |
| Micro WER | 0.0767 |
| Macro WER | 0.0754 |
| Mean polish score | 50.3 |
| ASR p50 / p95 (ms) | 27 / 63531 |
| Total p50 / p95 (ms) | 27 / 63531 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| fleurs-en | 50 | 1095 | 84 | 0.0767 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| fleurs-en-0011 | 0.3333 | 3 | 0 | 2 | through the night between 150 and 200 copies were made now k… | For the night between one fifty and two hundred copies were … |
| fleurs-en-0007 | 0.2857 | 4 | 1 | 1 | the aspect ratio of this format dividing by twelve to obtain… | The aspect ratio of this format, dividing by12 to obtain the… |
| fleurs-en-0020 | 0.2778 | 4 | 1 | 0 | but after losing the captain's wicket india only made 36 run… | But, after losing the captain's wicket India, only made36 ru… |
| fleurs-en-0033 | 0.2500 | 6 | 0 | 1 | during the struggle for independence organised by the mau mo… | During the struggle for independence organized by the Mao mo… |
| fleurs-en-0047 | 0.2500 | 3 | 0 | 1 | the announcement was made after trump had a phone conversati… | The announcement was made after Trump had a phone conversati… |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 28 |
| no_hallucination | 27 |
| bullets_when_listing | 1 |
| no_fillers | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 50 | 18 | 87724 | 4539 | 27 | 63531 | 87724 |
| total | 50 | 18 | 87724 | 4539 | 27 | 63531 | 87724 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0477 |
| Verbatim WER (macro) | 0.0457 |
| Entity accuracy | 0.2000 |

