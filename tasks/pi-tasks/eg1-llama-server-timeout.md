# Task: EG-1 llama-server killed by SIGKILL during model load (timeout too short)

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo. Do NOT touch
  the `enviouswispr/` reference tree or any sibling.
- Files you may edit: `crates/teletype-inference/src/server.rs`.
- In your final report, paste the REAL output of every verification command below, not prose.

## Why (the real gap)
Selecting EG-1 in the Models screen shows:
  "Download failed: Llama-server exited early with signal: 9 (SIGKILL) (log: .../teletype-llama-server-eg-1-63470.log)"

The log file for the failing PID is 0 bytes. But other EG-1 logs (PIDs 65243, 56629, 64443) show
the model loads fine and serves requests. The difference: the failing runs hit the `READY_TIMEOUT`.

Root cause: `READY_TIMEOUT` is 60s (server.rs:29). EG-1 is 8 shards totaling 2.7 GB. On a cold
disk cache, loading all 8 shards can exceed 60s. When `wait_until_ready` (server.rs:281-324)
times out at line 305-310, it returns an error. The caller at server.rs:108-111 then does
`child.kill()` (SIGKILL) + `child.wait()`. The 0-byte log is because llama-server hadn't flushed
stderr yet when it was killed mid-load.

The fix: increase `READY_TIMEOUT` so large multi-shard models have time to load. 60s is too
short for 2.7 GB on a cold cache. 120s is a safe upper bound (the working logs show model load
completes in ~2s on a warm cache, so 120s only matters for cold starts).

## Read first
1. `crates/teletype-inference/src/server.rs` — the whole file. Focus on:
   - Line 29: `const READY_TIMEOUT: Duration = Duration::from_secs(60);`
   - Lines 281-324: `wait_until_ready` (the polling loop)
   - Lines 108-111: the caller that kills the child on timeout
2. `crates/teletype-inference/src/catalog.rs` — the EG-1 catalog entry (8 shards, 2.7 GB) to
   confirm the model size.

## What to build
1. Change `READY_TIMEOUT` from 60s to 120s (server.rs:29):
       const READY_TIMEOUT: Duration = Duration::from_secs(120);
2. Add a comment above the constant explaining why 120s (large multi-shard models like EG-1
   at 2.7 GB can exceed 60s on a cold disk cache).
3. No other changes. The polling loop, health check, and kill-on-timeout logic are correct.

## Verification (run these, paste real output)
1. `cd /Users/a.sorathiya/Documents/Ali/teletype && cargo build -p teletype-inference 2>&1 | tail -20`
   (must compile, no errors).
2. `cd /Users/a.sorathiya/Documents/Ali/teletype && cargo test -p teletype-inference 2>&1 | tail -30`
3. `cd /Users/a.sorathiya/Documents/Ali/teletype && git diff crates/teletype-inference/src/server.rs`

## Out of scope (do NOT touch)
- The `wait_until_ready` polling logic (it's correct).
- The catalog, download, or model management code.
- The `enviouswispr/` reference tree.
