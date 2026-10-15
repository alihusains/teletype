# L026 — Editing a shell script while a background job runs it corrupts the running job

- **ID:** L026
- **Title:** bash reads scripts lazily by byte offset; editing the file mid-run makes the job abort with phantom syntax errors
- **Date:** 2026-10-15
- **Agent/tool:** OpenCode lead session (`feat/beat-wispr`)
- **Project area:** dev workflow / background jobs
- **Status:** verified
- **Evidence:** both `fetch_datasets.sh` background jobs (fleurs, ami) printed
  ``unexpected EOF while looking for matching `"'`` at line ~196 immediately
  after the script had been edited (commit `429e860`) while they were still
  running; `bash -n` passes on the committed file; per-set side effects
  (`.done`, manifests, 50/50 wavs per FLEURS language, 100/100 AMI) were
  complete before the abort.

## Problem

Two long-running background fetches finished their actual dataset work, then
aborted with a bash syntax error pointing at a quote near the end of a script
that parses cleanly (`bash -n` OK, re-runs fine).

## Root cause

Bash reads a running script incrementally, by byte offset. Editing the file
while the job executes (here: a commit that changed the script's line count
and byte offsets) makes bash re-read shifted bytes; it hit EOF inside a
quoted region and aborted with a misleading "unexpected EOF" diagnostic. The
message points at the script, but the script is fine — the *read position*
was stale.

## Rule

Never edit a shell script while a background job is executing it. If a job
must survive edits, copy the script to a temp path first and run that copy.
When a running job prints inexplicable syntax errors, check whether the file
was edited mid-run before debugging the script itself — and verify the job's
side effects (markers, manifests, output counts) rather than trusting the
exit status.

## Verification

`bash -n` on the committed script passes; the aborted jobs' outputs were
verified complete per set (.done + manifest + audio counts for all 10 FLEURS
languages and AMI).
