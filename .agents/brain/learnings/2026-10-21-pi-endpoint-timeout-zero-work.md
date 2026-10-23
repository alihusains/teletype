# Pi dispatch failed with pure request timeouts (zero work, misleading exit 0)

- **ID:** L-20261021-pi-endpoint-timeout
- **Date:** 2026-10-21
- **Agent/tool:** Qoder lead session + pi 1.3.0
- **Project area:** agent team / delegation tooling
- **Status:** candidate

## Problem
Three parallel `pi --mode json` dispatches (ui-dead-controls, model-recommend-backend,
engine-toggle-wiring) all exited 0 but produced **zero file edits**. Each hit
"Request timed out" on the very first model request and burned all 3 auto-retries.

## Symptoms
- Background task exit code 0 ("completed").
- events.jsonl ends with `auto_retry_end {success:false, finalError:"Request timed out."}`
  and an `aborted` event; the last "assistant" content is just the echoed input prompt.
- stderr only shows the session-creation warning.
- `find ui/src crates -newermt <dispatch time>` returns nothing.

## Initial hypotheses
1. Hang/deadlock in pi — rejected: process exited on its own after retries.
2. Concurrent-session contention — rejected: a single `pi --no-session -p "PROBE-OK"`
   probe with no other load also timed out. Model endpoint was simply unreachable.
3. CA/TLS issue (`--use-system-ca` / `NODE_EXTRA_CA_CERTS`) — possible contributor,
   untested; the same env works for normal ops elsewhere.

## Investigation steps
1. Read stderr + tail of events.jsonl per session (jq on `auto_retry|aborted` types).
2. `find` for files modified after dispatch time → none: no partial work to salvage.
3. One-shot probe: `pi --no-session -p "Reply with exactly: PROBE-OK"` → "Request timed out."

## Evidence
tasks/pi-tasks/logs/*-{events.jsonl,stderr.log} from 2026-10-21 19:51; probe output.

## Root cause
Upstream model endpoint unavailable to pi (free-tier provider) for the whole window;
retries exhausted before any tool call ran.

## Fix (workaround)
Fell back to in-session `general-purpose` agents with write access, reusing the same
self-contained task files (`tasks/pi-tasks/*.md`) and identical disjoint file ownership.

## Verification
Fallback agents completed the work (see task report in the same session's history);
pi itself produced no diff.
RESOLVED same day: user fixed the endpoint; post-fix probe
`pi --no-session -p "Reply with exactly: PROBE-OK"` returned PROBE-OK, exit 0.
Rule unchanged: probe before trusting dispatches after any endpoint incident; do not
re-dispatch in-flight work to pi (file-ownership conflict with the fallback agents).

## Reusable lesson
- Pi exit code 0 does NOT mean the task ran. Before trusting any pi "complete":
  check for `auto_retry_end success:false` / `aborted` in the JSONL and diff-by-mtime
  (`find <scope> -newermt <dispatch-time>`). Zero modified files = zero work, retry or
  fall back.
- When pi's endpoint is timing out, do not spam retries in parallel; one cheap
  `--no-session -p` probe decides it, then fall back to session agents.
- The task files themselves are the portable artifact: written for pi, they ran
  unchanged through the Agent tool. Keep writing them self-contained.
