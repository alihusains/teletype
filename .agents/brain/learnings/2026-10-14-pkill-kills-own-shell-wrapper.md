# L023 — pkill -f can kill your own shell wrapper instead of the target process

- **ID:** L023
- **Title:** pkill -f matches the killer's own command line; kill workers by PID
- **Date:** 2026-10-14
- **Agent/tool:** OpenCode lead session (phase3 S3)
- **Project area:** agent ops / background workers
- **Status:** candidate

## Problem
Needed to stop a background `pi` worker started via opencode `shell(background=true)`
whose command line contained `--session-id "teletype-phase3-s3-20261014a"`.

## Symptoms
`pkill -f "teletype-phase3-s3-20261014a"` reported success, and the opencode
background shell died with `Killed by SIGTERM` — but a `pgrep` showed a
surviving PID still carrying the session id. The kill hit the wrapper (whose
own command string contains the pattern), not necessarily the child.

## Investigation
- Read the shell's `.out` file: only `(no output)` + `Killed by SIGTERM`.
- `ps -p <pid>` on the surviving PID: it was already gone (`No such process`) —
  in this case the child died with its parent, and `git status` confirmed pi
  had written no code. Zero-loss stop, but only verifiable after the fact.

## Root cause
`pkill -f` matches full command lines, and the shell wrapper running the
`pkill` (or its parent) itself contains the pattern string. The match set
includes the killer.

## Fix / verification
1. Identify the worker PID precisely (`ps`/`pgrep -af <pattern>`, compare
   against your own shell PID).
2. `kill <PID>` (plain, no -f), then re-`ps` to confirm it is gone.
3. `git status --short` + `git diff --stat` to confirm what the worker did or
   did not change before deciding to keep/revert/continue.

## Reusable lesson
Never `pkill -f <unique-string>` from a shell whose own command line contains
that string. Kill by PID, verify by `ps`, then audit the tree.
