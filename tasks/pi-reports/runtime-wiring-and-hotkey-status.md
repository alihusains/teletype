# Runtime wiring: profiles, learning, and hotkey status

Status: implemented in the main working tree, uncommitted.

## What changed
- **AppState owns the bandit**: loads `polish_bandit.json` at startup; all writes go through one locked `update_bandit` helper.
- **Settle → promotion**: dictation rows now persist `polish_profile`; `settle::sweep` emits attributed bandit signals; `edit_watch::settle_sweep` replays them and calls `promote_profile`, so a learned change is finally recorded.
- **Feedback → bandit**: thumbs/undo outcomes feed the bandit only when a new feedback row is written; feedback now carries the take's profile.
- **Insights → bandit**: `compute_learning_with_bandit` supplies real `profileLearnings`; `undo_profile_learning(app_key)` applies a real `BanditState::undo_change`.
- **Profile → runtime**: the resolved profile is encoded into the existing instruction channel before `Pipeline::run` in both dictation paths. Precedence: explicit per-app override > learned bandit profile > global/category default.
- **Honest card data**: `polish-diff` now carries `profile` and `reason`; the Why popover shows real values or "unknown"/"none reported".
- **Hotkey liveness**: native `is_live()` getters, `hotkey_live`/`hotkey_physically_down`, a 2-sample dead-tap watcher with key-held/permission guards, `hotkey_status` + `reregister_hotkey` commands, and `hotkey-status` event.
- **UI**: Hotkey row in Permissions settings with Active/Inactive/Not-checkable states and a re-register action; Insights undo calls the new command.

## Verification
- `cargo test -p teletype-core`: 1376 lib + all integrations, 0 failed.
- `cargo test -p teletype-desktop`: 624 lib + 26 integration, 0 failed, 3 ignored.
- Eval self-test PASS; `tsc --noEmit` clean; vitest 58 files / 599 tests; `cargo fmt --all -- --check` clean.
- Brain freshness checks PASS; one expected warning for two untracked learning files. Learning L090 was deprecated after the wiring landed.

## Remaining gaps
- "Never change this" is stored as feedback but still not fed into the guard's protected-term input.
- Live Mac acceptance is needed for: a persisted `BanditChange` driving the next take, the Hotkey row flipping on a dead tap, and the Dock/Force-Quit crash report.
- Browser preview only reads hotkey status once; it does not push simulated `hotkey-status` events.
