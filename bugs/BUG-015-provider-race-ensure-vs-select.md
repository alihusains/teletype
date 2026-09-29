# BUG-015 — ensure_local_provider races select_openai_provider / select_model; stale provider can win

**Severity:** P2 (wrong output source, narrow race window)
**Area:** desktop / commands.rs
**Status:** open
**Found:** 2026-09-28 QA audit (5-agent sweep), verified by reading `commands.rs:1402-1468` vs `:1256-1305`

---

## Description

`ensure_local_provider` (`commands.rs:1402-1468`) is not atomic with
`select_openai_provider` / `select_model`.

Scenario: dictation starts while the user is switching to a remote API
provider. The dictation worker calls `ensure_local_provider` (checks
`inference.is_some()` → false, then builds + warms a `ServerProvider`
**outside the lock**, up to 120 s). Meanwhile `select_openai_provider`
completes its `inference.replace(remote)`. The worker then does
`inference.replace(local)` at `:1461`, **overwriting the remote provider the
user just selected**, and drops the remote one.

Symptom: user switches to OpenAI/Groq, next dictation silently uses the
local model (or the reverse). No error anywhere; transforms just come from
the "wrong" model.

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-desktop/src/commands.rs` | 1402-1468 | `ensure_local_provider` — builds outside the lock, replaces at `:1461` |
| `crates/teletype-desktop/src/commands.rs` | 1256-1305 | `select_openai_provider` — the racing replace |

## Reproduction

1. Start a dictation (triggers `ensure_local_provider`).
2. While it's warming (up to 120 s), switch to a remote OpenAI-compatible
   provider in Settings.
3. The dictation completes using the **local** model, not the remote one the
   user just selected.

## Unit test cases (must pass after fix)

```rust
// 1. Once a user explicitly selects a provider, a concurrent
//    ensure_local_provider cannot overwrite it.
#[test]
fn user_selection_wins_over_concurrent_ensure() {
    let state = test_state_with_no_provider();
    let handle = std::thread::spawn(move || {
        ensure_local_provider(&state); // slow warm-up path
    });
    // Simulate the user selecting a remote provider during the warm-up.
    select_openai_provider(&state, remote_config());
    handle.join().unwrap();
    assert!(state.current_provider_is_remote(),
        "ensure_local_provider overwrote the user's explicit selection");
}

// 2. ensure_local_provider re-checks the current provider under the lock
//    before installing.
#[test]
fn ensure_rechecks_under_lock_before_install() {
    let state = test_state_with_remote_provider_selected();
    ensure_local_provider(&state); // must be a no-op
    assert!(state.current_provider_is_remote(),
        "ensure_local_provider installed a provider the user had already chosen");
}
```

## Acceptance criteria

- [ ] An explicit user selection (`select_model` / `select_openai_provider`)
      is never silently overwritten by a concurrent `ensure_local_provider`.
- [ ] The provider used for a dictation is the one the user last selected.

## How to test (manual / smoke)

1. **Smoke (2 min):** start a dictation, immediately switch provider in
   Settings.
   - **Pass:** the dictation uses the provider that was current at
     injection time — i.e. the user's last explicit choice (document which
     one wins: the user's selection).
   - **Fail:** the dictation uses the local model even though the user just
     selected a remote provider.

2. **Regression (1 min):** normal single-provider flow.
   - **Pass:** dictation behaves unchanged when no provider switch happens
     mid-flight.
   - **Fail:** provider selection is lost or duplicated.

## Fix direction

Make provider install **atomic**: take the inference lock for the whole
check-and-install, and have `ensure_local_provider` re-check under the lock
whether a provider was already installed (by the user) since it started
building. If the user selected a provider during the warm-up, back off and
use the user's choice:

```rust
// after warm-up completes:
let mut lock = inference.lock().unwrap();
if lock.is_some() {
    return lock.take(); // user (or another worker) already installed one
}
*lock = Some(local_provider);
```

## Related

- BUG-007 (`select_model` ordering)
- BUG-013 (lifecycle)
