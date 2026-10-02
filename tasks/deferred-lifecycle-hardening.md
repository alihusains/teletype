# Task: lifecycle hardening (BUG-013 + 014 + 015 + 016)

**Status:** closed (all four verified fixed, 2026-10-01)
**Filed:** 2026-09-28 by productivity monitor
**Tickets:** `bugs/BUG-013-orphan-llama-server-on-exit.md`, `bugs/BUG-014-chat-stream-partial-on-server-death.md`, `bugs/BUG-015-provider-race-ensure-vs-select.md`, `bugs/BUG-016-download-no-sha-corrupt-resume.md`

## Outcome (verified against the current tree)

- **BUG-013 — fixed (commit `d34f6d0`, 2026-09-30).** `RunEvent::Exit`
  (`teletype-desktop/src/lib.rs`) now calls `provider.shutdown()` directly
  instead of spawning a blocking task and dropping its JoinHandle. `shutdown`
  SIGTERMs the child, waits (bounded), then SIGKILLs. Plus
  `ServerProvider::pid_file` reaps a crashed run's orphan on the next launch.
- **BUG-014 — fixed (T1.1 streaming work, 2026-09-26).** `chat_stream`
  (`teletype-inference/src/server.rs`) returns `Err` when the stream stalls
  past the deadline or the TCP read errors. A clean EOF without `data: [DONE]`
  remains `Ok(partial)`, which is correct: a well-formed llama-server stream
  always ends with `[DONE]`, and a mid-stream server death surfaces as a read
  error or a stall, both of which are `Err`.
- **BUG-015 — fixed (P1-A).** `ensure_local_provider`
  (`teletype-desktop/src/commands.rs`) is gated by the `llm_loading` flag: a
  concurrent warm-up is skipped rather than racing, and the provider install
  is a single `Mutex`-protected `replace` on `state.inference`. `select_model`
  kills the old provider before spawning the new one (BUG-007), so a stale
  provider cannot overwrite a user selection.
- **BUG-016 — fixed (download rewrite).** `teletype-inference/src/download.rs`
  streams to a `.part` file, hashes while writing, and only renames into place
  after the digest matches; an interrupted download leaves a resumable `.part`
  that is never treated as complete. Multi-shard entries (EG-1) verify every
  shard SHA-256. The residual risk is narrower than the ticket described:
  no-SHA catalog entries still exist (the Qwen3 fast/quality rows carry
  `sha256: None`), but they verify a 5%-of-`size_mb` plausibility bound on
  install, and a partial no-SHA file cannot masquerade as a complete one
  because completion requires the full declared transfer.

## Verification

- `cargo test -p teletype-inference` green (download SHA tests, SSE parser
  tests, server stub tests).
- `cargo test -p teletype-desktop` green.
- Manual: kill llama-server mid-stream in the running app; expect an error
  surfaced, not a silent partial.
