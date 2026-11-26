# Plan 002: Make the local-LLM loading guard atomic and hold it until install

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the STOP conditions section occurs, stop and report — do not improvise. When done, update the status row for this plan in plans/README.md unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: git diff --stat 8746c1c..HEAD -- crates/teletype-desktop/src/commands/models.rs crates/teletype-desktop/src/lib.rs
> If either file changed since this plan was written, compare the Current state excerpts against the live code before proceeding; on a mismatch, STOP.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit 8746c1c, 2026-11-24

## Why this matters

ensure_local_provider is called concurrently at startup: lib.rs:1369 spawns a thread from rehydrate_provider, and lib.rs:1375 calls ensure_local_provider on another thread. The guard at models.rs:1386-1398 reads llm_loading under one lock acquisition and writes true under a second, so two threads can both observe false before either writes. warm_up() then loads two multi-GB llama-servers at once — the exact doubled-RAM/OOM window BUG-007 was meant to close. The flag is also cleared at :1411 before the under-lock install decision at :1427-1447, opening a second spawn window while a provider is being installed.

## Current state

- crates/teletype-desktop/src/lib.rs:217 — pub llm_loading: Mutex<bool>, initialized at :393 and :1033. :1369 and :1375 are the two startup callers. models.rs:2584 and the runtime-status path READ the flag, so the guard must not be held as a MutexGuard across warm_up().
- crates/teletype-desktop/src/commands/models.rs:1336-1452 — pub fn ensure_local_provider(app: &AppHandle). Current guard and clear:

      {
          if *state.llm_loading.lock().unwrap_or_else(std::sync::PoisonError::into_inner) {
              return;
          }
          *state.llm_loading.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = true;
      }
      let provider = teletype_inference::ServerProvider::new(entry.id, entry.name, &path);
      let result = provider.warm_up();
      *state.llm_loading.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = false;
      if let Err(e) = result { /* log */ return; }
      { /* BUG-015: should_install_local re-checked under the inference lock
           if should_install_local(...) { inference.replace(Arc::new(provider)); }
           else { provider.stop(); return; }
      } }

- :1464 — fn should_install_local(current: bool, selected_provider: &str) -> bool; existing unit tests at :3013-3025 cover its three outcomes. Do not change this function.
- Repo convention: every lock use is .lock().unwrap_or_else(std::sync::PoisonError::into_inner). Match it exactly; no bare unwrap on poisoned locks.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Desktop crate tests | cargo test -p teletype-desktop | exit 0, 0 failures |
| Full suite | cargo test --workspace | exit 0, 0 failures |
| Format check | cargo fmt --all -- --check | exit 0 |

## Scope

In scope:
- crates/teletype-desktop/src/commands/models.rs

Out of scope (do NOT touch):
- crates/teletype-desktop/src/lib.rs — the two startup callers are correct once the guard is atomic.
- should_install_local and its tests at :1464, :3013-3025 — the BUG-015 decision logic stays as-is.
- rehydrate_provider, select_model, select_openai_provider, and all download/cancel paths.
- No settings field, IPC signature, or UI change.

## Git workflow

- Branch: advisor/002-atomic-llm-loading
- Commit message style: fix(models): make the llm_loading guard atomic through provider install
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Extract a single-lock claim helper

Add near ensure_local_provider in models.rs:

    fn claim_loading(flag: &std::sync::Mutex<bool>) -> bool {
        let mut loading = flag.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if *loading { return false; }
        *loading = true;
        true
    }

Replace the two-lock block at :1386-1398 with:

    if !claim_loading(&state.llm_loading) { return; }

Verify: cargo check -p teletype-desktop -> exit 0.

### Step 2: Clear the flag on every exit path, after install

Do NOT clear the flag at former line :1411. Instead, immediately after a successful claim, install a local RAII guard that clears on drop:

    struct ClearLoading<'a>(&'a AppState);
    impl Drop for ClearLoading<'_> {
        fn drop(&mut self) {
            *self.0.llm_loading.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = false;
        }
    }
    let _clear_loading = ClearLoading(&state);

State<AppState> derefs to AppState; pass &state (or &*state). Delete the explicit = false assignment so the flag stays set across warm_up() AND the should_install_local install section. The RAII guard clears it on success, on the warm_up error return, and on the provider-discarded return. Confirm no early return exists between the claim and guard creation.

Verify: grep -n llm_loading crates/teletype-desktop/src/commands/models.rs shows the claim helper, the RAII guard, and no standalone = false in ensure_local_provider. Then cargo check -p teletype-desktop -> exit 0.

### Step 3: Add a contention test for the helper

In the existing mod tests of models.rs next to the should_install_local tests (:3013-3025), add claim_loading_allows_exactly_one_caller: create one Mutex<bool>, spawn 8 threads each calling claim_loading(&flag), collect the booleans, assert exactly one true. Use the plain #[test] style of that module (no async).

Verify: cargo test -p teletype-desktop claim_loading_allows_exactly_one_caller -> 1 passed.

## Test plan

- New test: claim_loading_allows_exactly_one_caller in models.rs tests. Run it 5 times: for i in 1 2 3 4 5; do cargo test -p teletype-desktop claim_loading_allows_exactly_one_caller --quiet || exit 1; done
- Existing should_install_local_* tests must still pass unchanged — they verify the install decision this plan must not disturb.
- Verification: cargo test -p teletype-desktop -> all pass.

## Done criteria

- [ ] grep -n claim_loading crates/teletype-desktop/src/commands/models.rs shows one definition and one call in ensure_local_provider.
- [ ] grep -n llm_loading ... shows no direct read-then-write pair in ensure_local_provider.
- [ ] The explicit = false at former line :1411 is gone; a Drop implementation owns the clear.
- [ ] cargo test -p teletype-desktop and cargo test --workspace exit 0.
- [ ] Only models.rs and plans/README.md are modified.

## STOP conditions

Stop and report if:
- AppState is not in scope in models.rs as expected, or State<AppState> does not deref to AppState.
- A return path exists between the claim and the RAII guard creation.
- should_install_local behavior would need to change for this fix.
- A verification command fails twice after a reasonable fix attempt.

## Maintenance notes

- The flag is a Mutex<bool> because runtime status reads it; do not convert to AtomicBool without updating those readers.
- warm_up() can take up to 120 s. Holding the MutexGuard for that duration would block runtime-status reads, which is why the RAII guard only clears on drop and does not hold the lock.
- If a future call path adds an early return inside the install section, the RAII guard handles it automatically — that is the reason for the guard over explicit clears.
