# L030 — pi workers can burn their whole run exploring a large file and die before editing

- **ID:** L030
- **Title:** pi session `bw-p43` failed with zero edits after exhaustive exploration of a ~4000-line file; cap the exploration budget in the kickoff prompt
- **Date:** 2026-10-16
- **Agent/tool:** pi (`Qwen/Qwen3.8-27B-FP8` via icmarkets), dispatcher: OpenCode lead session (`feat/beat-wispr`)
- **Project area:** dev workflow / pi task dispatch
- **Status:** candidate (failure proven by evidence; the budget-cap mitigation is being validated by the `bw-p43r` retry)
- **Evidence:** `tasks/pi-tasks/logs/bw-p43-events.jsonl` — 34 turns, 40 tool executions, ALL read-only (30 bash greps/seds, 12 ranged reads, mostly `dictation.rs`), zero edit tool calls, final `message_end` contains a malformed `<function=todo>…</tool_call>` text instead of a tool call, then `agent_end`/`agent_settled`; 138 487 input tokens consumed. `git status` in the worktree showed no changes to the two files the worker owned.

## Problem

A pi worker asked to implement P4.3/P4.4 (edit `committer.rs` + `dictation.rs`) produced no changes at all. It spent its entire context exploring the controller file (~4000 lines) — repeatedly reading large ranges and grepping for adjacent concepts — and then emitted a malformed pseudo-tool-call as text and stopped.

## Symptoms

- Background pi shell exits "successfully" (no stderr error, no crash); only the event log shows the abnormal ending.
- Worktree is untouched for the worker's owned files.
- The final assistant message is a broken function-call string (`<function=todo>\n<parameter=create>\n</function>\n</tool_call>`), not a valid tool call or report.
- Very large input-token count for a single session (138k) with no edits.

## Initial hypotheses

1. pi transport broke again (like the 14 Oct empty-response deviation). — Rejected: two sibling workers in the same wave had real edits in the same worktree at the same time.
2. The task file was too vague. — Rejected on reflection: the two other workers used the same task-file format and edited successfully; the failure is budget, not clarity.
3. The model exhausted context/budget on exploration of a large file and degraded into malformed output. — Consistent with all evidence.

## Investigation steps

1. Read the last 14 event-log entries — saw the malformed final message and `agent_settled`.
2. Extracted every tool call from the log: 30 bash (all read-only) + 12 reads, 0 writes/edits.
3. Compared against sibling worker logs (`bw-p5core`, `bw-p6`) which had normal edit flows at the same time — transport healthy.
4. Checked the main checkout and worktree `git status` for stray writes — none.

## Root cause

No formal root cause from pi, but the evidence pattern is: unbounded exploration of a large file consumed the effective budget and the session terminated before the first edit. Task files that say "read first: <2000+ line file>" without a budget invite this. It is not (this time) a transport failure and not a task-scope failure — workers that finished had smaller primary files.

## Fix (provisional, being validated)

Re-dispatch with budget rules written into the kickoff prompt:

- never read files whole; use `grep -n` then ranged reads (≤ 300 lines each, ≤ 6 ranged reads);
- after at most 10 search/read operations, start editing;
- explicitly restate the owned-file list and the deliverable.

## Verification

`bw-p43r-<hhmmss>` dispatched 16 Oct with the budget prompt; its event log + worktree diff will confirm or reject the mitigation. Promote this learning to `verified` only if the retry produces edits; otherwise record the observed failure distribution across workers (how many sessions died with zero edits) and revisit.

## Reusable lesson

A pi "success" exit says nothing about whether work happened. Always check (a) the event log's final message for malformed/absent tool calls and (b) the worktree diff for the owned files before treating a worker as merely "still running" or "done". When a task's primary file is huge, cap the exploration budget in the kickoff prompt itself.
