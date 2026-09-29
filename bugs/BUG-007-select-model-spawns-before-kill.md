# BUG-007 — select_model spawns the new llama-server before killing the old (doubled RAM, failed model switch)

**Severity:** P1 (model switch can silently fail, leaving the old model active)
**Area:** desktop / commands.rs
**Status:** open
**Found:** 2026-09-28 QA audit (5-agent sweep), verified by reading `commands.rs:1157-1196`

---

## Description

In `select_model` (`crates/teletype-desktop/src/commands.rs:1141-1196`), the new
provider is built and `warm_up()`-ed inside `spawn_blocking` at `:1157-1171`
**before** the code takes the `inference` lock and `replace()`-es the old one at
`:1177-1182`. The old provider is only dropped **after** the new one is
confirmed ready (`:1187-1189`).

So between spawn and replace, **both** llama-servers are alive. For EG-1 that is
~5.4 GB of weights + KV cache, times two.

### Scenario

On a memory-strapped laptop (16 GB), the new server's load competes with the
still-alive old server. The OS OOM-kills the new one. The command blocks up to
120 s (`READY_TIMEOUT`) waiting for readiness that never comes, and because the
`replace()` at `:1177-1182` never runs, the user is left with the **old** model.

### User sees

"Select did nothing" — they picked model B, waited ~2 minutes, and model A is
still active. No error is surfaced for the timeout; the switch just fails
silently.

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-desktop/src/commands.rs` | 1157-1171 | `spawn_blocking` warm_up of the NEW provider (before old is killed) |
| `crates/teletype-desktop/src/commands.rs` | 1177-1182 | `inference.replace()` — old dropped only after new is ready |
| `crates/teletype-desktop/src/commands.rs` | 1187-1189 | old provider dropped off the async runtime |

## Reproduction

1. Load model A (e.g. EG-1).
2. In Settings, select model B.
3. On a 16 GB Mac, watch Activity Monitor — **two llama-server processes appear
   simultaneously**.
4. If RAM is tight, the new one is killed, the command hangs ~120 s, and model A
   is still active.

## Unit test cases (must pass after fix)

```rust
// 1. The old provider must be killed BEFORE the new one is warmed.
#[test]
fn old_provider_killed_before_new_warm_up() {
    // MockProvider records the order of spawn / warm / kill calls.
    let mut recorder = CallRecorder::new();
    let old = MockProvider::new(&mut recorder, "model-a");
    let mut order = vec![];
    // Simulate select_model: kill old, then spawn+warm new.
    kill_provider(&mut recorder, &old);
    warm_up_new(&mut recorder, &mut order);
    assert!(order.iter().position(|e| e == Kill).unwrap()
        < order.iter().position(|e| e == Warm).unwrap(),
        "kill(old) must happen before warm(new) — otherwise two 5GB servers are alive at once");
}

// 2. At most one provider is alive at any point during a model switch.
#[test]
fn at_most_one_provider_alive_during_switch() {
    let mut state = ProviderState::new();
    state.load("model-a");
    assert_eq!(state.alive_count(), 1);
    state.start_switch("model-b");
    // While the switch is in flight (new not yet ready), count must never exceed 1.
    assert!(state.alive_count() <= 1,
        "doubled RAM: both servers alive during switch");
    state.finish_switch("model-b");
    assert_eq!(state.alive_count(), 1);
    assert_eq!(state.active_model(), "model-b");
}
```

## Acceptance criteria

- [ ] The old provider is killed (child process terminated) **before** the new
      one is spawned/warmed.
- [ ] At most one llama-server process is alive at any point during a model
      switch.
- [ ] `select_model` does not block 120 s on a memory-strapped machine (no
      doubled-RAM OOM window).
- [ ] The "Select did nothing" symptom is gone: a failed switch surfaces an
      error instead of silently keeping the old model.
- [ ] `cargo test -p teletype-desktop` passes.

## How to test (manual / smoke)

1. **Smoke (2 min):**
   - Load model A. Select model B.
   - After the switch completes, run `pgrep -fl llama-server`.
   - **Pass:** exactly ONE llama-server process, running model B.
   - **Fail:** two processes, or model A still active.

2. **Memory pressure (5 min):**
   - On a 16 GB machine, load model A, then switch to model B.
   - **Pass:** switch completes with one server; no OOM kill.
   - **Fail:** ~120 s hang, model A still active.

3. **Regression (2 min):**
   - Switch A → B → A → B → A (3 round trips).
   - **Pass:** exactly one server after each switch, no orphan processes.
   - **Fail:** orphan llama-server processes accumulate.

## Fix direction

Reorder `select_model` so the old provider is dropped (its llama-server child
killed) **before** the new one is spawned and warmed. Accept a brief window
with no LLM available — transforms fall back to their input, which is safe —
rather than a window with two ~5 GB servers resident.

Concretely: take the `inference` lock, drop the old provider (kill child),
release the lock, then `spawn_blocking(warm_up)` the new provider, then lock
again and `replace()`. Surface the `READY_TIMEOUT` failure as a user-visible
error so a failed switch is never silent.

## Related

- BUG-013 — orphan llama-server on Exit (same child-lifecycle area)
- BUG-015 — provider race with `ensure_local_provider`
