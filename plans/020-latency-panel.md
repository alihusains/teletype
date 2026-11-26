# Plan 020: Show per-stage inference latency in the Developer tab

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 8746c1c..HEAD -- crates/teletype-desktop/src/dictation.rs ui/src/screens/DeveloperScreen.tsx crates/teletype-core/src/speech/mod.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: M
- **Risk**: LOW
- **Depends on**: none
- **Category**: direction
- **Planned at**: commit `8746c1c`, 2026-11-26

## Why this matters

The app runs a multi-stage pipeline: audio capture → ASR (parakeet/whisper)
→ ITN → polish (local LLM via llama-server) → delivery. The Insights screen
shows aggregate latency percentiles (per-stage, all-time), but the Developer
tab (the power-user / debugging surface) does not show the latency of the
*last* dictation. When a user reports "that last dictation felt slow," the
developer cannot see which stage was slow. Adding a "last dictation
breakdown" panel to the Developer tab (ASR ms, ITN ms, polish ms, delivery
ms, total ms) makes latency debugging immediate.

The data is already collected: `history` table has `polish_ms`,
`duration_ms`, and the pipeline logs per-stage timings via `log_entry`.
The gap is that the Developer tab does not display them.

## Current state

- `crates/teletype-core/src/persist.rs:109-121` — `history` table has
  `duration_ms`, `polish_ms` columns.
- `crates/teletype-desktop/src/dictation.rs` — the dictation pipeline logs
  per-stage timings. Search for `log_entry` calls with timing info to find
  where ASR/ITN/delivery durations are measured.
- `ui/src/screens/DeveloperScreen.tsx` — the Developer tab. Check what
  panels it currently shows (model status, log ring, settings overrides).
- `crates/teletype-desktop/src/commands/` — check for an existing
  `get_last_dictation` or `get_latency_stats` command.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Build | `cargo build -p teletype-desktop` | exit 0 |
| Desktop tests | `cargo test -p teletype-desktop --lib` | all pass |
| Typecheck | `cd ui && npx tsc --noEmit` | exit 0 |
| UI tests | `cd ui && npm test` | all pass |

## Scope

**In scope**:
- `crates/teletype-desktop/src/dictation.rs` — add a `last_dictation_timings`
  field to the state (or use the existing `history` table)
- `crates/teletype-desktop/src/commands/` — add `get_last_dictation_timings`
  Tauri command
- `ui/src/screens/DeveloperScreen.tsx` — add a "Last dictation" panel
- `ui/src/` — IPC binding

**Out of scope**:
- Any change to the dictation pipeline logic
- Any change to the Insights screen (it already shows aggregate stats)
- Any change to the audio or inference crates

## Git workflow

- Branch: `advisor/020-latency-panel`
- Commit message: `feat(dev): show per-stage latency breakdown in Developer tab`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Identify where per-stage timings are measured

Search `dictation.rs` for timing measurements:
```
grep -n "Instant::now\|elapsed\|duration_ms\|polish_ms\|asr_ms" crates/teletype-desktop/src/dictation.rs | head -20
```

Identify where ASR, ITN, polish, and delivery durations are measured. They
may already be stored in a struct or logged via `log_entry`. If they are
only logged (not stored), add a `LastDictationTimings` struct:

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct LastDictationTimings {
    pub asr_ms: u64,
    pub itn_ms: u64,
    pub polish_ms: u64,
    pub delivery_ms: u64,
    pub total_ms: u64,
    pub timestamp: u64,
    pub text_preview: String, // first 80 chars
}
```

Store it in `AppState` (behind a `Mutex` or `ArcSwap`) so the Tauri command
can read it.

**Verify**: you have identified where each stage's duration is measured (or where it needs to be added).

### Step 2: Add the Tauri command

In `crates/teletype-desktop/src/commands/` (find the appropriate file), add:

```rust
#[tauri::command]
pub async fn get_last_dictation_timings(
    state: State<'_, AppState>,
) -> CommandResult<Option<LastDictationTimings>> {
    Ok(state.last_dictation_timings.lock().unwrap().clone())
}
```

Register it in `generate_handler!` in `lib.rs`.

**Verify**: `cargo build -p teletype-desktop` → exit 0.

### Step 3: Add the Developer tab panel

In `ui/src/screens/DeveloperScreen.tsx`, add a "Last dictation" panel:
- Call `invoke("get_last_dictation_timings")` on mount and on a 2-second
  interval (or on a "Refresh" button).
- Display: ASR (ms), ITN (ms), Polish (ms), Delivery (ms), Total (ms).
- Show the text preview (first 80 chars) and timestamp.
- Show "No dictation yet" if `null`.
- Use a simple table or stat grid (match the existing Developer tab style).

**Verify**: `cd ui && npx tsc --noEmit` → exit 0.

### Step 4: Run the test suites

**Verify**: `cargo test -p teletype-desktop --lib` → all pass.
`cd ui && npm test` → all pass.

## Test plan

- The panel renders "No dictation yet" before any dictation.
- After a dictation, the panel shows the per-stage breakdown.
- `cargo test -p teletype-desktop --lib` → all pass.
- `cd ui && npm test` → all pass.

## Done criteria

- [ ] `cargo build -p teletype-desktop` exits 0
- [ ] `cargo test -p teletype-desktop --lib` passes
- [ ] `cd ui && npx tsc --noEmit` exits 0
- [ ] `cd ui && npm test` passes
- [ ] `LastDictationTimings` struct is defined and populated during dictation
- [ ] `get_last_dictation_timings` Tauri command is registered
- [ ] Developer tab shows the per-stage latency panel
- [ ] No files outside the in-scope list are modified
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The per-stage timings are not measured anywhere in the pipeline (you would need to add `Instant::now()` calls at each stage boundary — that is a larger change than this plan assumes).
- The `AppState` structure differs from the expected shape (the timings field may need to go in a different location).
- A step's verification fails twice after a reasonable fix attempt.

## Maintenance notes

- The timings are in-memory only (lost on app restart). If persistence is needed, write them to the `history` table (the `polish_ms` and `duration_ms` columns already exist; add `asr_ms`, `itn_ms`, `delivery_ms` columns).
- The 2-second poll interval is conservative. If the Developer tab is only opened when debugging, a manual "Refresh" button is sufficient.
