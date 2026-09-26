---
name: ali-pi-team
description: Launch the Teletype agent team. When Ali gives a task, break it into well-scoped units, delegate the mechanical/parallelizable ones to pi (background), do the small/quick ones inline, and verify every pi result before reporting done.
trigger: /ali-pi-team
---

# ali-pi-team — the Teletype build team

Turn a task into a dispatched work plan. This is the "agent team" for Teletype:
the primary session (you) + a pool of `pi` background workers. Use it whenever
Ali hands over a build task and wants throughput.

## When to use
- Ali gives a feature/fix task that is bigger than a one-line change.
- The task decomposes into 2+ independent units (different files/dirs = safe to parallelize).
- Do NOT use for: security-sensitive work, production credentials, or a change so small the task file costs more than doing it.

## The loop

### 1. Decompose first
- Read the task. Split into units where each unit:
  - touches a **disjoint set of files** (overlapping files → sequence, never parallel).
  - can be described in one self-contained task file.
- Classify each unit:
  - **MECH** — mechanical, multi-file, well-specified → delegate to pi.
  - **QUICK** — one function / few lines / needs your judgment → do inline now.
- Decide the split explicitly before dispatching.

### 2. Write one task file per MECH unit
Path: `tasks/pi-tasks/<slug>.md`. Every file MUST have:
- **How to work this task** block: name the repo (`/Users/a.sorathiya/Documents/Ali/teletype`),
  fence it ("work ONLY inside this repo"), and require REAL command output in the report.
- **Why** — the real gap, with evidence (file:line, a doc quote).
- **Read first** — the exact files to read before writing code (highest-leverage section).
- **What to build** — specific enough that two runs produce the same thing.
- **Verification** — the exact commands (`cargo build -p <crate>`, `cargo test -p <crate>`,
  `npx tsc --noEmit` in `ui/`) and an instruction to paste their real output.
- **Out of scope** — what NOT to touch.

### 3. Dispatch each as its own background process
```bash
cd /Users/a.sorathiya/Documents/Ali/teletype
export NODE_OPTIONS="--use-system-ca"
pi -p "Read and execute the task in tasks/pi-tasks/<slug>.md in full — it starts with a
'how to work this task' block, follow it strictly: work ONLY inside
/Users/a.sorathiya/Documents/Ali/teletype, stay in scope, read every named source file
before writing code, and paste the REAL output of every verification command in your
final report." < /dev/null > tasks/pi-tasks/logs/<slug>.log 2>&1
```
- Run in background; one process per task; log to `tasks/pi-tasks/logs/<slug>.log`.
- Parallel only tasks with no file overlap.

### 4. Keep working while pi runs
- Do all the QUICK units inline now. Don't idle on the background notification.

### 5. Verify before reporting done (call /ali-verify-pi-work)
- A pi "done" is a claim, not a fact. Re-run its verification commands, read the real
  diff, check for crash artifacts. Only then mark done.

## Teletype-specific guardrails
- After any `cargo build`, if `llama-server` is touched, re-sign it:
  `codesign --force --sign - target/debug/llama-server` (L006 — prevents SIGKILL).
- UI changes: verify with `cd ui && npx tsc --noEmit` (exit 0).
- Tauri IPC is camelCase (`selectedLlmModel`, not `selected_llm_model`).
- Restart/reload the app yourself when needed — never ask Ali to do it.

## Output
Report: units + MECH/QUICK split, which were dispatched (task ids), which done inline,
and the verification status of each.
