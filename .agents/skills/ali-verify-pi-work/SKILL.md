---
name: ali-verify-pi-work
description: Never trust a pi "done" report. Verify pi's claimed work by re-running its verification commands, reading the real diff, and checking for crash artifacts before marking a task done.
trigger: /ali-verify-pi-work
---

# ali-verify-pi-work — prove pi's work before you trust it

`pi` (the background coding agent) crashes mid-task and still leaves real work behind,
and it reports "done" on incomplete work. A "done" report is a claim, not a fact.
Run this after every pi task before you tell Ali it's done.

## The checks (in order)

### 1. Check for crash-left work BEFORE deciding anything
A "failed"/"timed out" exit does NOT mean nothing happened.
```bash
cd /Users/a.sorathiya/Documents/Ali/teletype
git status --short
git diff --stat
```
You may already have the work, just without a final report. Read the diff before retrying or discarding.

### 2. Compile / typecheck first (cheap, catches breakage)
```bash
cargo build -p <crate> 2>&1 | tail -5        # for Rust changes
cd ui && npx tsc --noEmit; echo "EXIT: $?"   # for UI changes (must be 0)
```

### 3. Run the real tests
```bash
cargo test -p <crate> 2>&1 | tail -8
```
Confirm the pass count matches what the report claimed.

### 4. Read the actual diff (not the report)
Look specifically for crash-retry artifacts:
- **Duplicate** function/struct/route definitions (a retry re-added what an earlier crash already wrote).
- **Broken string literals** from a botched edit (concat syntax left inside a quoted block).
- **Scope creep** into files the task excluded.
- **Stale comments/docstrings** still describing the pre-task state.

### 5. Exercise the real behavior when it's a feature
- Spin up the dev app (see /ali-app-dev-loop) and hit the exact behavior the task claimed.
- For a backend claim: make a real call, inspect the real result.

### 6. If you find a bug the crash left
- Fix it yourself (you have full context; it's usually 2 lines). Don't just re-run pi.

## Verdict
Only report a pi task **done** after you personally reproduced its claimed verification.
State: what you re-ran, the real output, and the diff verdict.

## Teletype specifics
- Rust: `cargo build -p teletype-desktop` / `teletype-core` / `teletype-inference`.
- UI: `cd ui && npx tsc --noEmit` → exit 0.
- If `llama-server` was rebuilt: `codesign --force --sign - target/debug/llama-server` (L006).
