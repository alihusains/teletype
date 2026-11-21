# Optimizations and v0.2.16 hotkey failure

Status: implemented in the main working tree, uncommitted.

## Optimizations applied
- `crates/teletype-core/src/insights.rs`: reuse the already-computed `changed_word_count` for zero-edit detection; fold the repeated history scans into one accumulation pass with cached word counts and normalized tokens. All existing window semantics preserved; added two regression tests.
- `crates/teletype-core/src/learning/feedback.rs`: seed an in-memory dedupe-key set once from the JSONL file instead of reparsing it on every feedback click. Key remains `(take_id, kind, term, span)`.
- `crates/teletype-core/src/learning/profile_bandit.rs`: `save()` no longer writes an empty body on serialization failure; it serializes first and writes via a sibling temp file + rename.
- `ui/src/pill.tsx`: memoize `diffWords`/`changeGroups`/`changeCount` on `[raw, polished]` so card button clicks do not rerun the LCS.
- `crates/teletype-desktop/src/commands/calibration.rs`: resolve the shared packet/settings/transforms once before the 3×3 sample fan-out; keep calls sequential because one local model is used.
- `crates/teletype-core/src/transforms/backtrack.rs`: pre-split cue words into static arrays; consolidate duplicated `NUM_WORDS` tables.

## Hotkey failure fix
Root cause traced: a lost key-release edge can leave both the native and Rust down-latches set. The native latches were only partially reset, and the Rust latches in `fn_tap.rs`/`mod_tap.rs` were never reset on re-registration, so later presses were discarded as repeats. CMCP-1-style silent tap delivery loss is a plausible trigger.

- New `crates/teletype-desktop/src/hotkey_latch.rs`: `EdgeLatch` resets on start/stop and treats a down state older than 30 s as a fresh press.
- `fn_tap.rs` / `mod_tap.rs`: use `EdgeLatch`; serialize native stop/start with a lifecycle mutex to avoid double-join.
- `fn_tap.m` / `mod_tap.m`: clear `g_fnDown` / `g_down` whenever a disabled tap is re-enabled.
- `input_engine_tap.m`: add the missing 2 s tap watchdog.

Regression tests were written before the fix (compile failure), then passed 2/2 after implementation.

## Verification
- `cargo test -p teletype-core`: 1373 lib + all integration suites, 0 failed.
- `cargo test -p teletype-desktop`: 609 passed, 3 ignored, 0 failed (temporary `binaries/llama-server-win/` placeholder created and removed).
- Polish eval self-test PASS; `tsc --noEmit` clean; vitest 57 files / 587 tests; `cargo fmt --all -- --check` clean.
- Brain freshness: PASS checks, with an expected warning for two untracked learning files (this learning plus the earlier T5 learning). No commits were made.

## Remaining uncertainty
The Dock-icon unresponsiveness and Force Quit crash were not reproducible on this host and no OS crash report was available. They are consistent with an unresponsive main thread; this fix removes a proven hotkey-latch failure mode but does not prove the crash is eliminated. Please provide the `.ips` crash report from `~/Library/Logs/DiagnosticReports` if it recurs after rebuilding.
