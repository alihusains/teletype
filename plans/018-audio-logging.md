# Plan 018: Replace raw eprintln! in production audio error callbacks with tracing

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 8746c1c..HEAD -- crates/teletype-core/src/audio/mod.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: dx
- **Planned at**: commit `8746c1c`, 2026-11-26

## Why this matters

Three audio stream error callbacks in `crates/teletype-core/src/audio/mod.rs`
use raw `eprintln!` to report stream errors. These are **production code**
(not `#[cfg(test)]`, not env-gated) and run in the shipped binary. Raw
`eprintln!` in a Tauri app means: (a) the message goes to the process stderr,
which is captured by the OS but not by the app's log ring (the Developer tab
and Sentry breadcrumbs use the structured `log_entry`/`tracing` path, not
stderr); (b) in a release .app bundle, stderr is typically discarded, so the
error is invisible to the user and to crash reporting. The repo already has a
structured logging path (`tracing` + `log_entry`) that feeds the log ring and
Sentry breadcrumbs — these three callbacks should use it.

**Note**: the other `eprintln!` occurrences found in the audit are either
`#[cfg(test)]` (harmless — not compiled in release) or env-gated debug probes
(`TELETYPE_DELIVERY_AUDIT`, `TELETYPE_ITN_TRACE`) which are by-design
developer tools. This plan targets only the three production, non-gated
`eprintln!` calls in `audio/mod.rs`.

## Current state

- `crates/teletype-core/src/audio/mod.rs:705` —
  ```rust
  let err_fn: Box<dyn Fn(cpal::Error) + Send + 'static> = Box::new(move |err| {
      eprintln!("[audio] stream error: {err}");
      err_flag.store(true, Ordering::SeqCst);
  });
  ```
- `crates/teletype-core/src/audio/mod.rs:1289` —
  ```rust
  Box::new(move |err| eprintln!("[audio] prewarm stream error: {err}"))
  ```
- `crates/teletype-core/src/audio/mod.rs:1394` —
  ```rust
  Box::new(move |err| eprintln!("[audio] prewarm stream error: {err}"))
  ```

**Logging convention**: the repo uses `tracing` for structured logging.
Check how other error paths in `audio/mod.rs` log — search for
`tracing::error!` or `log_entry` in the same file. Match the existing
pattern. If `audio/mod.rs` already uses `tracing::error!` elsewhere, use the
same macro. If it uses `log_entry`, use that.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Build | `cargo build -p teletype-core` | exit 0 |
| Core tests | `cargo test -p teletype-core --lib audio` | all pass |
| Clippy | `cargo clippy -p teletype-core -- -D warnings` | exit 0 |

## Scope

**In scope** (the only file you should modify):
- `crates/teletype-core/src/audio/mod.rs`

**Out of scope** (do NOT touch):
- `crates/teletype-core/src/delivery.rs` — the `eprintln!` there is env-gated (`TELETYPE_DELIVERY_AUDIT`), by design
- `crates/teletype-core/src/transforms/backtrack.rs` — the `eprintln!` there is `#[cfg(test)]`
- `crates/teletype-core/src/itn.rs` — the `eprintln!` there is `#[cfg(debug_assertions)]` + env-gated
- `crates/teletype-core/src/rescoring.rs` — the `eprintln!` there is `#[cfg(test)]`
- Any other file

## Git workflow

- Branch: `advisor/018-audio-logging`
- Commit message: `fix(dx): use tracing for production audio stream error callbacks`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Identify the logging pattern used in audio/mod.rs

Search `audio/mod.rs` for existing structured logging:
```
grep -n "tracing::\|log_entry\|eprintln\|println" crates/teletype-core/src/audio/mod.rs
```

Determine which macro the file already uses for error-level logging. If it
uses `tracing::error!`, use that. If it uses `log_entry(LogLevel::Error, ...)`,
use that. If it uses neither (only `eprintln!`), use `tracing::error!` (it is
the repo-standard structured logger and feeds both the log ring and Sentry).

**Verify**: you have identified the logging macro to use.

### Step 2: Replace the three eprintln! calls

Replace each `eprintln!` with the identified logging macro, preserving the
message content:

Line 705:
```rust
// Before:
eprintln!("[audio] stream error: {err}");
// After (example, adjust macro to match Step 1):
tracing::error!("[audio] stream error: {err}");
```

Lines 1289 and 1394:
```rust
// Before:
Box::new(move |err| eprintln!("[audio] prewarm stream error: {err}"))
// After:
Box::new(move |err| tracing::error!("[audio] prewarm stream error: {err}"))
```

**Important**: the closures are `Fn(cpal::Error) + Send + 'static`. The
`tracing::error!` macro is `Send`-safe (it does not capture non-Send state).
If the logging macro you identified in Step 1 is not `Send`-safe (e.g. it
captures a `&mut` reference), use `tracing::error!` instead — it is always
safe in `Send` closures.

**Verify**: `cargo build -p teletype-core` → exit 0.

### Step 3: Verify no production eprintln! remains in audio/mod.rs

```
grep -n "eprintln\|println" crates/teletype-core/src/audio/mod.rs | grep -v "test\|cfg"
```

**Verify**: no matches (all remaining `eprintln!`/`println!` are in `#[cfg(test)]` or env-gated code).

### Step 4: Run the audio test suite

**Verify**: `cargo test -p teletype-core --lib audio` → all pass.

### Step 5: Run clippy

**Verify**: `cargo clippy -p teletype-core -- -D warnings` → exit 0.

## Test plan

- The three error callbacks are exercised by the audio test suite (stream
  error injection tests). Confirm they still pass.
- No new tests needed — the change is a logging-macro swap, not a logic change.
- `cargo test -p teletype-core --lib audio` → all pass.

## Done criteria

- [ ] `cargo build -p teletype-core` exits 0
- [ ] `grep -n "eprintln\|println" crates/teletype-core/src/audio/mod.rs | grep -v "test\|cfg"` returns no matches
- [ ] `cargo test -p teletype-core --lib audio` passes
- [ ] `cargo clippy -p teletype-core -- -D warnings` exits 0
- [ ] No files outside the in-scope list are modified (`git diff --name-only`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The logging macro identified in Step 1 is not `Send`-safe and `tracing::error!` is not available (check `Cargo.toml` for the `tracing` dependency — it should be there; if not, STOP).
- Replacing `eprintln!` with the structured logger changes the closure's type in a way that breaks compilation (e.g. the logger captures a non-Send reference).
- A step's verification fails twice after a reasonable fix attempt.

## Maintenance notes

- If new audio stream error callbacks are added, use the same structured logging macro (not `eprintln!`).
- The env-gated debug probes (`TELETYPE_DELIVERY_AUDIT`, `TELETYPE_ITN_TRACE`) are by-design developer tools — do not replace them with structured logging (they are intentional stderr output for debugging).
- If the app's log ring or Sentry integration changes, verify the audio error messages still appear in the log ring.
