# Teletype Eval Report

- **Git SHA:** ab53b08
- **Timestamp:** 1790515914
- **Model:** none
- **Mode:** replay

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 17 |
| Micro WER | 0.0296 |
| Macro WER | 0.0428 |
| Mean polish score | 91.2 |
| ASR p50 / p95 (ms) | 380 / 1100 |
| Total p50 / p95 (ms) | 1650 / 3800 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| awkward | 2 | 30 | 0 | 0.0000 |
| filler | 3 | 41 | 10 | 0.2439 |
| list | 2 | 34 | 0 | 0.0000 |
| long | 1 | 119 | 0 | 0.0000 |
| multi | 1 | 19 | 0 | 0.0000 |
| names | 2 | 25 | 0 | 0.0000 |
| numbers | 2 | 33 | 0 | 0.0000 |
| plain | 3 | 28 | 0 | 0.0000 |
| question | 1 | 9 | 0 | 0.0000 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| filler-02 | 0.3077 | 0 | 4 | 0 | I mean basically the build is green and actually all the tes… | the build is green and all the tests pass |
| filler-03 | 0.2667 | 0 | 4 | 0 | So um like the standup is at nine and er I need to be there | the standup is at nine and I need to be there |
| filler-01 | 0.1538 | 0 | 2 | 0 | Um the deploy went out this morning and uh it is all good | the deploy went out this morning and it is all good |
| awkward-01 | 0.0000 | 0 | 0 | 0 | The semicolon in the SQL query is the one that is causing th… | the semicolon in the SQL query is the one that is causing th… |
| awkward-02 | 0.0000 | 0 | 0 | 0 | We need to upgrade from version two to version three before … | we need to upgrade from version two to version three before … |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_content_loss | 3 |
| bullets_when_listing | 1 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 |
|---|---|---|---|---|---|---|
| asr | 17 | 280 | 1100 | 431 | 380 | 1100 |
| total | 17 | 1400 | 3800 | 1822 | 1650 | 3800 |

