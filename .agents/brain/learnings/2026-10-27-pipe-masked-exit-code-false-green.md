---
id: L079
title: A piped verification command reports the tail's exit code, not the checker's - near-miss false green on the clippy gate
status: verified
type: verification-discipline
area: .agents workflows / lead verification of pi workers
agent: main session (Qoder)
tool: bash, cargo clippy, scripts/check-clippy-clean.py
date: 2026-10-27
---

# A piped verification command reports the tail's exit code, not the checker's

## Problem

While verifying the `vocs-u4b-families-consumers` worker under the repo's
"verify the claim, a settled worker is not proof of work" rule, I re-ran the
clippy cleanliness gate to confirm the worker left no lints behind.

## Symptoms

The command printed `gate exit=0` for a run that had just listed 11 clippy
warnings. Had I believed it, U4 would have been recorded as clippy-clean and the
gate would have gone red later for a reason nobody could attribute.

## Initial hypotheses

1. `scripts/check-clippy-clean.py` treats those 11 as non-blocking warnings and
   exits 0 by design.
2. The script silently failed and printed nothing usable.
3. My own command was measuring the wrong process.

## Investigation steps

1. Re-read the printed list: the entries were `unused_constants` / `dead_code` /
   `matches!` / `unnecessary filter` - real lints with real `code` fields, so
   hypothesis 1 was unlikely for a gate whose whole job is to fail on them.
2. Re-ran the pipeline splitting it into stages so the checker's status was the
   last thing the shell saw:
   `cargo clippy ... --message-format=json > /tmp/clip.json; python3 scripts/check-clippy-clean.py < /tmp/clip.json > /tmp/clip.out; echo "gate exit=$?"`
   Result: **`gate exit=1`**, with the identical 11 warnings.
3. Confirmed the difference between the two forms: `... | python3 gate.py; echo $?`
   reports the exit status of the **last pipeline element** only when the shell's
   pipefail option is off the status propagates oddly, and in my first command the
   checker's output was itself piped into `tail -3`, so `$?` was `tail`'s status.
   The second form removed every downstream pipe.

## Evidence

Same clippy JSON, two commands, two exit codes: `0` when the checker was followed
by a pipe into `tail`, `1` when its status was captured directly.

## Root cause

My verification command, not the code. `echo $?` after `cmd | tail` reports
`tail`, which almost always succeeds. The gate was red the whole time: 11 lints
across `fillers/corpus.rs`, `fillers/tiers.rs` (the live `vocs-u5b` worker's
files) and `trace.rs` (the live `vocs-u2c` worker's file).

## Fix

No code fix. Command form fixed, and the finding itself was real and useful:
the workspace clippy gate is currently red **because of in-flight worker files**,
while `transforms/**` (the unit under verification) is clean. That distinction is
what the final report to Ali carries instead of a blanket "clippy green".

## Verification

`cargo clippy -p teletype-core --all-targets --message-format=json > f.json; python3 scripts/check-clippy-clean.py < f.json; echo $?`
-> `1`, and the per-file attribution above. The U1 reachability test and the U4
transforms suite were then re-run in the same un-piped form.

## Reusable lesson

When a verification command's exit code is the evidence, capture it on the
command itself: redirect to a file and `echo $?` immediately, or use
`set -o pipefail`. Never let `tail`, `head`, or `grep` sit between the checker and
`$?`. And when a gate is red during parallel work, attribute the failures by file
before drawing a conclusion about the unit under review - a red workspace gate is
not evidence that the unit you are verifying is broken, and reporting it as such
would send the next session to fix someone else's in-flight file.
