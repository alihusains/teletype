---
id: L081
title: A worker's bulk replace over struct literals produces two defects that lib-only test runs cannot see
status: verified
type: verification
area: teletype-core/pipeline + teletype-desktop/commands + worker verification routine
agent: main session (Qoder), verifying the vocs-u7c worker
tool: cargo clippy --all-targets, after cargo test --lib had already passed
date: 2026-10-28
---

# Bulk `s.replace()` across struct literals: a gate the lib tests miss

## Problem

U7c had to add `filler_strength` to the `Pipeline` struct, which means every struct
literal that constructs a `Pipeline` must gain the field. That is 4 production sites and
roughly 30 test/bench literals across `pipeline.rs`, `journey_smoke.rs`, `i18n.rs`,
`concurrency_stress.rs`, `benches/pipeline.rs` and `self_test.rs`. The worker did it with
Python `s.replace(old, new)` passes over whole files instead of per-site edits.

## Symptoms

Both survived the worker's own verification, and survived mine until I widened the gate:

1. `crates/teletype-core/tests/journey_smoke.rs:189` declared the field's **type** as
   `teletype_core::fillers::tiers::FillerStrength::Fillers`, a variant path, because the
   replace inserted the identical text into the struct declaration and the value slot.
   A compile error: `expected type, found variant`.
2. The same file's `dictate()` hardcoded `FillerStrength::Fillers` rather than reading
   `self.filler_strength`, so the new `Fixture` field was set in `Fixture::new()` and
   never consumed: `field filler_strength is never read`. Dead config, the exact defect
   class the settings-reachability guard (U1) exists to catch, reintroduced inside a test.

## Investigation steps and evidence

- `cargo test -p teletype-core --lib` and `cargo test -p teletype-desktop --lib` both
  green (926 and 409). The corpus still reported 1.0000. Every guard test passed:
  `settings_reachability`, `ipc_contract` 11/11, `migration` 18/18. `npx tsc --noEmit`
  clean and vitest 311 green.
- The worker never ran `--all-targets`, so benches and the four integration test
  binaries were never compiled by anyone until the lead ran the clippy gate.
- The write footprint came from the events JSONL, not git: 12 `edit` hunks, all inside
  owned files, plus shell/Python rewrites that touched `fillers/tiers.rs` (another
  unit's file) and the five test/bench files it had not declared. The task file was the
  bug there: it listed the production files and omitted the struct-literal fallout, so
  the worker went looking for the missing sites itself.

## Root cause

A textual replace has no notion of a type position versus a value position, and it will
not rewrite a use site to read a field it just added. Lib-only test selection then hides
the result entirely, because the files affected are integration tests and benches.

## Fix

- Line 189 became the type; `dictate()` now passes `self.filler_strength`.
- Added three mapping assertions to
  `commands::tests::filler_strength_wire_values_round_trip` (`Light -> Hesitations`,
  `Standard -> Fillers`, `Strict -> Full`), which the worker had not written: it asserted
  `Unknown -> Standard` only, so swapping the Light and Strict arms would have shipped.
- The stray probe modules the worker appended to `fillers/tiers.rs` were already removed
  by its own truncation; re-running the filler suite (20 passed, corpus 1.0000) proved
  nothing was lost, which is luck rather than a check.

## Verification

`cargo clippy -p teletype-core -p teletype-desktop --all-targets` clean (784 crates, 0
warnings). `cargo test -p teletype-core` all 12 binaries green (926 lib, 62 journey_smoke,
11 ipc_contract, 6 i18n, 2 concurrency_stress). `cargo test -p teletype-desktop` green
(409 lib, 18 migration, 1 settings_reachability). The Light/Standard/Strict control was
then read back in the browser: group labelled "How much filler to remove", clicking Strict
moves `aria-checked` from Standard to Strict.

## Reusable lesson

- After any worker that added a **struct field**, the gate is `--all-targets`, not
  `--lib`. Benches and integration tests are where the literals live, and they are
  invisible to a lib-only run.
- Then check whether the new fixture fields are **read**, not just set. Clippy's
  `field is never read` is the same dead-config bug U1 guards against, one layer down.
- Dispatch files must enumerate the struct-literal fallout sites, or the worker will
  discover them with a bulk replace. Better: name the new field's owner files and tell
  the worker to run `cargo check --all-targets` and fix every literal it reports.
- When a new enum maps to an existing one, require assertions on **every** arm. A test
  that only covers the fallback case leaves a swapped mapping silent.
