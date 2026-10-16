# L033 — Repetition-loop death: identical commands repeated until the session burns out with an empty final message

- **ID:** L033
- **Title:** A worker session can loop on the same command indefinitely and end with an empty final message after thousands of events, producing zero output
- **Date:** 2026-10-16
- **Agent/tool:** lead session (OpenCode) orchestrating `pi` workers; provider `icmarkets`, model `Qwen/Qwen3.8-27B-FP8`; the death hit `bw-c2a`, a session launched with `pi -ne` (built-in tools only)
- **Project area:** tooling / pi worker orchestration
- **Status:** verified (2026-10-16 — waves C/D/D.5/E ran ~30 worker units under the mandatory ANTI-LOOP RULE with zero repetition-loop recurrences; the deaths that DID recur were the payload class, see L032)

## Problem

`bw-c2a` (P9 CI hardening) ran for ~40 minutes and consumed 4,606 events / 308 tool executions — and wrote **nothing**. All of its target files were missing afterwards.

## Symptoms

- The last ~13 recorded tool calls are the **identical command**: `grep -rn "hotword" crates/teletype-core/src/dictionary.rs | head -50` repeated verbatim.
- The final assistant message is **empty**: `content: []`, `stopReason: "stop"`, `usage.output: 0`, then `agent_end` → `agent_settled`.
- No error event, no stderr error; the session looks "completed" to a naive audit.
- Same empty-final-message signature also observed on `bw-c7-r2` (which had done real work before dying), so an empty final message is a generic silent-death marker, not loop-specific.

## Hypotheses / investigation

1. Payload-drop (L032) at a big write — rejected: no write was attempted; the loop was pure reads.
2. Provider stream cut (L032 class) — partially supported: the session ends cleanly with an empty message.
3. Model repetition collapse — **supported**: two consecutive identical tool results normally break a sane loop; here the worker re-emitted the same call at least 13 times. Sessions launched with `-ne` (no extension tools, no todo/plan scaffolding) appear more prone to this.

## Root cause

A repeated-tool-call collapse in the model loop: the same command is re-emitted, produces the same result, and the session expends its entire budget without progress, ending with an empty final message. Launching with `-ne` (already discouraged, see L031) likely compounds it.

## Fix (in force)

Mandatory ANTI-LOOP RULE appended to every `bw-*` task file:

1. Never run the same command twice when the second result adds nothing; two consecutive identical results → STOP that line of investigation.
2. Before each command ask: what NEW information will this produce?
3. A build/test showing the SAME foreign error is re-run at most once per sleep cycle.
4. Keep a short checklist of remaining steps; work one item per action.

Plus: all workers launch WITH the full extension toolset (never `-ne`).

## Verification

Verified 2026-10-16: after the anti-loop rule was appended to every `bw-*` task file (and launches moved off `-ne`), waves C, D, D.5 and E ran ~30 worker units; no session repeated a command into burn-out. The silent deaths that re-appeared in these waves belong to different classes (payload drop / truncated XML-as-text — L032) and their victims had real partial work on disk, unlike the original pure loop death. Audit rule stands: an empty final message is never evidence of success.

## Reusable lesson

- "Zero text + agent_settled" is NOT success — audit the last tool calls for repetition before treating a session as finished.
- A repetition loop can burn 4.6k events with zero artifact writes; wall-clock and event counts are not progress evidence.
- Anti-loop instructions belong in every worker task file in this environment.
