# Teletype Eval Report

- **Git SHA:** 19b2767
- **Timestamp:** 1793036611
- **Model:** fluid-speech-mini
- **Mode:** measure

> **WARNING:** This corpus contains synthesised audio
> (`human: false`). WER numbers computed over synthesised
> speech validate the harness plumbing and are **not** a
> statement about Teletype's accuracy on human speech.

## Metrics

| Metric | Value |
|---|---|
| Cases | 200 |
| Micro WER | 0.0211 |
| Macro WER | 0.0237 |
| Mean polish score | 78.8 |
| ASR p50 / p95 (ms) | 166 / 425 |
| Total p50 / p95 (ms) | 166 / 425 |
| Model load (ms) | 149 |

## WER by Tag

| Tag | Cases | Ref words | Errors | WER |
|---|---|---|---|---|
| competing-speaker | 200 | 4126 | 87 | 0.0211 |
| librispeech | 200 | 4126 | 87 | 0.0211 |
| ssr-10db | 200 | 4126 | 87 | 0.0211 |

## Worst 5 Cases

| ID | WER | S | D | I | Reference | Hypothesis |
|---|---|---|---|---|---|---|
| competing-10db-0035 | 0.2222 | 2 | 0 | 2 | goobers don't grow on the tops of vines but underground on t… | Gubels don't grow on the tops of vines, but on the ground, o… |
| competing-10db-0047 | 0.2222 | 2 | 0 | 0 | now you'll stay cried van say polly won't you | Now you'll stay, cried Anne. Stay, Polly, won't you? |
| competing-10db-0194 | 0.2083 | 5 | 0 | 0 | i couldn't shiver much bein bound so tight but when i'm loos… | I didn't shiver much being bound so tight, but when I loose … |
| competing-10db-0101 | 0.2000 | 1 | 0 | 0 | if i can get patients | If I can get patience, |
| competing-10db-0130 | 0.2000 | 1 | 0 | 0 | i boldly lighted my cheroot | I boldly lighted my tread, |

## Polish Failed Checks

| Check | Failures |
|---|---|
| no_hallucination | 46 |
| no_content_loss | 45 |
| terminal_punctuation | 41 |
| sentence_cased | 14 |
| no_fillers | 7 |

## Latency

| Series | Count | Min | Max | Mean | p50 | p95 | p99 |
|---|---|---|---|---|---|---|---|
| asr | 200 | 87 | 593 | 202 | 166 | 425 | 485 |
| total | 200 | 87 | 593 | 202 | 166 | 425 | 485 |

## Extended Metrics (P0.4)

| Metric | Value |
|---|---|
| Verbatim WER (micro) | 0.0207 |
| Verbatim WER (macro) | 0.0230 |
| Entity accuracy | 0.9000 |

