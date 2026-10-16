# L032 — Large tool-call payloads are silently dropped by the Qwen endpoint; chunk every big write

- **ID:** L032
- **Title:** A single tool call with a payload above ~5 KB / ~120 lines is silently cut by the provider — the worker session ends mid-write with no error
- **Date:** 2026-10-16
- **Agent/tool:** lead session (OpenCode) orchestrating `pi` workers; provider `icmarkets`, model `Qwen/Qwen3.8-27B-FP8`
- **Project area:** tooling / pi worker orchestration
- **Status:** candidate (mitigation verified in-flight via protocol change + relaunches; keep as candidate until a full wave completes under the chunked protocol)

## Problem
pi workers repeatedly ended mid-task exactly when starting a large file write, with no error and no report.

## Symptoms
- Assistant message ends with an announcement like "Now I'll write `judge.rs`:" — `stopReason: stop`, no tool call, immediately followed by `agent_end` / `agent_settled`.
- No error event, no stream error in stderr; session looks "completed".
- Bursts kill several workers around the same phase (first big file of the unit).
- Small writes succeed (2-byte probe, `wc`-style checks); a 5.8 KB `diff.rs` and a 12.7 KB `h2h.py` succeeded in some runs — reliability degrades with payload size, not a hard cutoff.
- With `pi -ne` (basic tools only) the same stoppage appears as the model emitting `<function=write> ...` XML-ish text instead of a tool call (see L031).

## Hypotheses tested
1. pi-artifact extension break — fixed/removed (L031), failure persisted → not the cause of this class.
2. Context window pressure — rejected: deaths occur at similar early session sizes as successes.
3. Provider silently truncates long tool-call streams — **supported**: the stream always ends at the announcement boundary right before the large payload would start; usage counters show zeros on deltas.

## Evidence
- `bw-c1b-r2-events.jsonl` tail: `text_end` "…Now I'll write `judge.rs`:" → `message_end` with a single text part, no toolCall → `turn_end` → `agent_end` → `agent_settled`.
- Same signature in `bw-c1a-r2` ("…write the complete `store.rs`.") and `bw-c8-r2` ("Now I'll write the files.").
- Disk state: the announced files were absent/90-byte stubs; earlier smaller files from the same sessions were intact.

## Root cause
The model endpoint cuts or fails to assemble very large tool-call payloads; pi receives a clean `stop` with the tool call missing and treats the turn as finished.

## Fix (in force)
Mandatory chunked WRITE PROTOCOL appended to every worker task file:
1. `write` at most ~120 lines per call (skeleton with TODOs is fine).
2. Append further chunks (≤ ~120 lines) via `bash` heredoc `cat >> file <<'PIEOF'`.
3. `wc -l` after each chunk; keep the file compiling where feasible.
4. On relaunch, resume from the file on disk — never rewrite from scratch.
5. Prefer several small `edit` calls over one huge replacement.

## Verification
Probe write worked; `diff.rs` (5.8 KB) content verified intact after a death; r3 workers relaunched with the protocol and state notes. Full verification = first unit to complete its big file under the chunked protocol.

## Reusable lesson
- Treat "session ends silently right after announcing a write" as a payload-size problem, not a logic problem.
- Worker task files must always carry a chunked-write protocol for this environment.
- Relaunch kickoffs must carry an explicit on-disk state summary so a resumed worker never rewrites completed files.

## Amendment 2026-10-16 (wave D.5/E) — second signature + the inspection rule

Recurrences on that date (c28-first, c15, c18, c31, c22, c19-attempt-2) added a variant
signature: the worker log's **last bytes are a literal truncated `<function=...>` XML
tool call emitted as TEXT** (e.g. `<function=bash><parameter=command>`), then silence and
no report — versus the original "announcement, then clean stop with no tool call". Both
are the same class: the provider cut the large tool-call stream and the session ended
looking "completed".

Operational rule that came out of it (now standard practice):

1. A worker that ends with zero/truncated output is a **silent death**, never "done".
2. ALWAYS `git status` + inspect the claimed files BEFORE relaunching — in most cases
   (c18, c31, c22, c19-attempt-2) the files landed (fully or partially) and the lead
   takes over verification/completion instead of re-running the unit from scratch.
3. A silent death can leave the tree RED mid-edit (c19-attempt-2: a call site landed
   without its method). Treat a red tree as evidence of a partial landing, not as a
   reason to revert.
4. Relaunch kickoffs must carry an explicit on-disk state summary (`git diff` ranges,
   exact compile error, what remains) so the resumed worker never rewrites what exists.
