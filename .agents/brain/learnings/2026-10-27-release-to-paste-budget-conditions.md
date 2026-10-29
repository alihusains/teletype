---
id: L080
title: The shipped 700 ms release-to-paste budget holds to 1.27x on short takes on the current engine, and the hand probe that overstated it was wrong
status: verified
type: measurement
area: teletype-core/trace + teletype-desktop/dictation
agent: main session (Qoder), cross-checked against the vocs-u2c worker's probe
tool: the real `latency_summary` over the on-disk trace JSONL, cargo test trace
date: 2026-10-27
---

# The shipped 700 ms budget is 1.27x out on short takes on the current engine

## Problem

`evals/thresholds.json` has carried `latency_release_to_paste_p95_ms_max = 700` since
the eval harness existed, but nothing in the running app ever measured the stage it
names. The number was a CI-corpus assertion, not an observation of what a user
waits for. U2 (the Vocs per-stage latency port) added the measurement; this is what
the first real data says.

## Symptoms

The panel's own headline row, over the 200 most recent takes on the author's
machine: `Release to paste p50 882 ms, p95 3648 ms` against a 700 ms budget.

## Investigation steps

1. The worker reported these numbers from a probe over
   `~/Library/Application Support/com.teletype.app/traces/*.jsonl`.
2. Recomputed them independently rather than trusting the report.
3. Decomposed `Total` into its two children to attribute the gap instead of just
   reporting it.
4. Later, stopped recomputing by hand and called the real `latency_summary` over
   the real store from a throwaway test, because step 2 had a bug (below).

## Evidence

Over the 200 newest takes, from the Rust aggregator itself:

| Stage | marks | n | p50 | p95 |
|---|---|---|---|---|
| Capture | `t_hotkey_down` -> `t_first_audio` | 185 | 1902 ms | 27687 ms |
| Transcribe | `t_hotkey_up` -> `t_text_pipeline_end` | 131 | 831 ms | 5645 ms |
| Polish (first token) | `t_polish_start` -> `t_polish_first_token` | 83 | 92 ms | 464 ms |
| Polish (full) | `t_polish_start` -> `t_polish_end` | 82 | 200 ms | 1250 ms |
| Inject | `t_text_pipeline_end` -> `t_paste_sent` | 123 | 58 ms | 198 ms |
| Release to paste | `t_hotkey_up` -> `t_paste_sent` | 117 | 882 ms | 3648 ms |

Capture is not a cost: it is how long the key was held, i.e. how long the user
spoke. It sits in the panel as context and is not part of release-to-paste, which
starts at hotkey **up**.

## Root cause

Injection is not the problem: 58 ms median, 198 ms p95. The entire budget overrun
sits between releasing the hotkey and the text pipeline finishing, i.e. ASR decode
plus the deterministic pass. The 700 ms threshold was set against a short, clean
eval corpus on one fast engine.

## Two conditions the 700 ms was measured under, not one

### Length

Splitting the same window by `audio_ms`:

| Condition | n | p50 | p95 |
|---|---|---|---|
| Short take (audio under 5 s) | 82 | 625 ms | 1399 ms |
| Long take (5 to 15 s) | 25 | 1584 ms | 2443 ms |
| Very long take (15 s and up) | 10 | 3649 ms | 6978 ms |
| Every length | 117 | 882 ms | 3648 ms |

The length effect is real and large (3648 to 1399 ms when long takes are excluded),
but it does not explain the overrun away.

### Engine

The same short takes, split by the `engine` each take recorded:

| Engine | n | p50 | p95 | its own transcribe p50/p95 |
|---|---|---|---|---|
| `parakeet-ultra-coreml` | 41 | 407 ms | 892 ms | 229 / 481 ms |
| `fluid-speech-mini` | 12 | 669 ms | 897 ms | 327 / 482 ms |
| `parakeet-tdt-v3` | 29 | 1056 ms | 1618 ms | 599 / 1070 ms |

`selectedSpeechModel` is `parakeet-ultra-coreml`, and every tagged take recorded
today ran on it. So the honest statement of the gap is **892 ms against a 700 ms
budget, 1.27x over, on 41 short takes on the engine actually in use**, not "2.2x
over". The engine mix in the history was inflating the figure by 57 percent
(1399 ms).

## Fix

No code fix to the latency itself here. Three fixes to the claim:

1. `get_latency_summary` now calls `derive_stage_ms()` on every trace it loads
   (`commands/system.rs`). Persisted `stage_ms` only exists for takes recorded
   after U2 shipped, so without this the panel would have shown "no timings yet"
   against eight days of perfectly good history.
2. The budget is stated against the condition it measures. `LatencySummary`
   carries `short_take_p95_ms`, `short_take_count` and
   `short_take_audio_ms_max` (`SHORT_TAKE_AUDIO_MS = 5000`), and `LatencyPanel`
   draws the warning colour from the short-take figure only. The table's "Release
   to paste" row still shows every length, and is no longer tinted against a
   short-clip budget it was never comparable to.
3. The short-take figure is scoped to one engine (`SHORT_TAKE_ENGINES`,
   `SHORT_TAKE_ENGINE_MIN_TAKES = 20`, `short_take_engine`): the engine of the
   **newest** short take in the window, when at least 20 short takes ran on it.
   Below that floor it stays engine-blind rather than reading a p95 off five
   takes, and the panel says "across all engines" instead of naming one. The
   verdict now renders as its own figure line (`892 ms` in the warning or success
   colour) rather than a sentence appended to the table, because the number is
   the answer to the promise.

## Verification

- `cargo test -p teletype-core --lib trace`: 20 passed, including
  `latency_summary_exact_percentiles_and_absent_stage`,
  `derive_stage_ms_rejects_non_monotonic`,
  `short_take_p95_excludes_long_takes_and_unmeasured_audio`, and the new
  `short_take_p95_scopes_to_the_newest_engine_when_enough_takes_back_it` (same
  data, reversed: 20 takes on one engine scopes to 500 ms, 5 takes on the newest
  engine refuses to scope and the slow takes dominate again).
- The per-engine and per-length tables above came from a throwaway
  `tests/tmp_latency_probe.rs` that called `read_recent_traces` +
  `latency_summary` on the real store and printed the result; it reproduced the
  worker's total row exactly (117 / 882 / 3648) and was deleted after reading.
- `cd ui && npx tsc --noEmit`: exit 0.
- The panel was read back as pixels in the browser preview, not inferred from a
  typecheck: `?latency` (scoped verdict in the warning colour: "892 ms p95 to
  paste, on parakeet-ultra-coreml, takes under 5 seconds (41 takes). Over the 700
  ms budget."), `?latency=within` (640 ms, computed colour `rgb(22, 163, 74)`,
  "Within the 700 ms budget"), `?latency=sparse` (polish unmeasured renders a
  hyphen with `role="img" aria-label="not measured"`, never a zero, and the
  unscoped copy "across all engines"), and no flag (the empty state sentence
  instead of a blank card).

## Reusable lesson

A threshold in `evals/thresholds.json` is a claim about CI fixtures, not about
users, **and it is a claim about a condition as well as a number**. This one sat
under two conditions: short clips, and one engine. Splitting by length turned "5x
over, alarming" into a real number; splitting by engine turned "2.2x over" into
1.27x. Separate the conditions before proposing work, because each one points at a
different fix (short-take transcribe speed versus long-take throughput).

**A hand-written probe of this store was wrong, and quietly.** The first version
of this file reported short-take p95 1551 ms over 92 takes. The probe sorted the
dated files ascending and iterated each file's lines forward, so its "newest 200"
was the **oldest** 200 rows plus a wrong within-file order. `read_recent_traces`
does `files.sort(); files.reverse()` **and** `text.lines().rev()`: both reversals
are required, and getting either one wrong still produces a plausible-looking
number rather than an error. The fix is not a more careful probe; it is to call
`read_recent_traces` + `latency_summary` from a throwaway test and print the
result, so the measurement and the shipped code cannot disagree.

Two smaller transferable details: the trace `t_*` marks are **microseconds**, so
any percentile computed by hand must divide by 1000; and a p95 over ~40 samples is
carried by two takes, so report the sample count next to the percentile.

Also: this number will move when the default engine or the typical take length
moves, so re-read the panel rather than re-reading this file.
