# Phase 3 — self implementation tracker (no pi; lead + subagents in parallel)

Branch: `refactor`. Baseline: HEAD `3a510a9` + uncommitted QUICK S6 tracing edits
(`core/src/audio/mod.rs`, `core/src/autotext/placeholders.rs`).

## Ownership (disjoint file sets — parallel-safe)

| Unit | Owner | Files | Status |
|---|---|---|---|
| S3 typed settings enums + `CommandError{code,message}` | LEAD (inline) | `desktop/src/commands.rs`, `desktop/src/lib.rs` (adapt only), tests in `commands.rs` | IN PROGRESS |
| A2 wiring (trace command + spans + waterfall) | LEAD (inline) | `desktop/src/dictation.rs`, `dictation_stages.rs` (Serialize), `lib.rs` (traces field), `commands/system.rs` (get_dictation_trace), `ui/.../DeveloperScreen.tsx` | DONE + VERIFIED (workspace green, 11 contract, 90 vitest, tsc clean) |
| A7 pill reducer `usePillMachine` | SUBAGENT | `ui/src/` only | DONE + VERIFIED (tsc clean, 85 vitest pass) |
| Arch doc refresh | SUBAGENT | `docs/architecture.md` only | DONE + VERIFIED (no stale claims) |
| UI error follow-up (`errorMessage` helper, S3 fallout) | LEAD (inline) | `ui/src/` only | DONE + VERIFIED (tsc clean, 90 vitest pass) |
| commands.rs domain split | SUBAGENT | `desktop/src/commands.rs`, NEW `desktop/src/commands/`, `lib.rs` handler list (read-only), `core/tests/ipc_contract.rs` scanners | DONE + VERIFIED (118 lib, 11 contract, clippy warning-set identical, lib.rs untouched) |
| KNOWN FLAKE (watch) | — | desktop lib suite: 1 failure in 11 runs (~9%), name not captured; green in 10 consecutive reruns | LEAD to identify on next recurrence (suspects: A2 `transform_stage_with_mock_engine_polishes` gate/validator sensitivity, or dictation cancel-ordering) |
| tauri-specta bindings (S4/A5) | LEAD | workspace + `ui/src` generated | AFTER split |
| teletype-platform extraction (A6) | LEAD | `crates/` layout | AFTER split |
| tracing Layer → Developer log (S6 rest) | LEAD | `desktop/src/commands.rs` log ring | AFTER S3 |

## Rules
- No one touches another owner's files. A2: NO edits to `commands.rs`/`lib.rs`
  (trace store accessor only; lead wires the command). UI: NO `.rs` edits.
  Docs: NO code edits.
- Wire JSON values byte-identical (S3 enums). Behavior-identical (A2 stages, pill).
- Verification per unit before "done"; lead runs `/ali-verify-pi-work`-equivalent
  review on subagent diffs (read diff + rerun scoped tests).
