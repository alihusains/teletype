# L031 — pi sessions need the full extension toolset; a broken extension kills every session at startup

- **ID:** L031
- **Title:** pi worker sessions: broken `pi-artifact` extension blocked all sessions; `pi -ne` is not a safe workaround (model emits XML tool calls as text at large writes)
- **Date:** 2026-10-16
- **Agent/tool:** lead session (OpenCode) orchestrating `pi` 1.3.0 workers
- **Project area:** tooling / pi worker orchestration
- **Status:** candidate (verified operationally in-session; keep as candidate until reproduced on a later day)

## Problem
All new `pi` sessions failed to start: `Failed to load extension ".../node_modules/pi-artifact/index.ts": Failed to load extension: (0, _piKit.resolvePackageDir) is not a function`.

## Symptoms
- Every new session exits at startup with hint "Start without extensions using `pi -ne`".
- With `-ne` (built-in tools only: read/bash/edit/write), workers execute reads/small edits, then die mid-task at their first **large `write`** by emitting the tool call as visible text (`<function=write> <parameter=content> ...`), which the harness logs as a final assistant message → `agent_end` → session over. 8/8 workers died this way. Same failure class as `bw-p43` (`<function=todo>` as text) — see L030's neighborhood.

## Initial hypotheses
1. pi was auto-updated and regressed stream parsing — **rejected**: `/opt/homebrew/bin/pi` binary dated 15 Oct, version 1.3.0 throughout.
2. Model-side change on the free endpoint — **rejected**: the long-running session (launched before the break, extensions loaded) kept working through the same window.
3. `-ne` changes behavior — **confirmed as a contributing factor**: sessions with the full extension toolset (44 tools) worked for hundreds of calls, including large writes; `-ne` sessions failed at large writes.

## Investigation steps + evidence
- `pi --version` → 1.3.0 (binary mtime 15 Oct). Extension dir mtime 18:15 today; npm dir churned at 18:22.
- `pi list` → `pi-artifact` installed today; `pi update npm:pi-artifact` claims "Updated" but the load error persists (same incompatible API call).
- `pi -ne` sessions: events show `agent_end`/`agent_settled` right after a text-emitting pseudo-tool-call; zero tool_execution for the write.
- Probe after `pi remove npm:pi-artifact`: `pi -p "write probe-out.txt"` → file created, reply `DONE`. Extension-loaded sessions healthy again.

## Root cause
`pi-artifact` (installed 16 Oct 18:15) calls `piKit.resolvePackageDir`, which does not exist in pi 1.3.0's pi-kit. Extension load is fatal for the whole session. The `-ne` workaround starves the model of the richer tool schemas and it then serializes large tool calls in a training-data XML dialect that the harness treats as plain text, silently ending the task.

## Fix
`pi remove npm:pi-artifact` (reversible with `pi install npm:pi-artifact` once a compatible release exists). Do NOT run pi workers with `-ne` as a general workaround; prefer removing/disabling only the broken extension.

## Verification
`pi -p` write probe succeeded (file content `OK`, reply `DONE`); 8 relaunched workers started with the full toolset and progressed.

## Reusable lesson
- A broken pi extension is a **global** outage: every new worker session dies at startup. Treat "new sessions fail instantly" as an extension problem first; check `pi list` + extension mtimes.
- `pi -ne` is not a safe fallback for this model: large `write` payloads get emitted as XML text and silently end the session (no error, no report). If `-ne` must be used, instruct workers to write large files via `bash` heredocs instead of the `write` tool.
- Probe (`pi -p` small write) before relaunching a batch of workers after any tooling change.
