# Plan 015: Align tauri-plugin-updater JS and Rust versions and add a CI pin check

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 8746c1c..HEAD -- crates/teletype-desktop/Cargo.toml ui/package.json Cargo.lock ui/package-lock.json`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: dx
- **Planned at**: commit `8746c1c`, 2026-11-26

## Why this matters

The updater is the one Tauri plugin where a JS/backend version mismatch has
direct user impact: the JS `check()`/download path talks to the Rust command
surface installed by the backend crate. Currently the JS side is 2.13.3 and
the Rust side is 2.13.0. All other Tauri plugin pairs are exactly aligned
(dialog 2.7.3/2.7.3, opener 2.5.5/2.5.5, process 2.4.0/2.4.0). The repo has
no check comparing the JS and Rust plugin versions — the same class of drift
that `check-artifact-pins.sh` was written to catch for llama.cpp. The next
`npm update` or `cargo update` moves exactly one side.

## Current state

- `ui/package.json:18` — `"@tauri-apps/plugin-updater": "^2.13.3"`
- `ui/package-lock.json` — resolves to `2.13.3`
- `crates/teletype-desktop/Cargo.toml:55` — `tauri-plugin-updater = "2"` (caret, resolves to 2.13.0 in Cargo.lock)
- `Cargo.lock` — `tauri-plugin-updater 2.13.0`
- All other plugin pairs are aligned: dialog 2.7.3, opener 2.5.5, process 2.4.0

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Bump Rust dep | `cargo update -p tauri-plugin-updater` | Cargo.lock shows 2.13.3 |
| Typecheck | `cd ui && npx tsc --noEmit` | exit 0 |
| UI tests | `cd ui && npm test` | all pass |
| Pin check | `bash scripts/check-artifact-pins.sh` | exit 0 |

## Scope

**In scope** (the only files you should modify):
- `crates/teletype-desktop/Cargo.toml` (pin the updater version)
- `Cargo.lock` (updated by cargo)
- `scripts/check-artifact-pins.sh` (add plugin version check)

**Out of scope** (do NOT touch):
- `ui/package.json` or `ui/package-lock.json` — the JS side is already at 2.13.3
- Any Rust source file
- Any other Cargo.toml

## Git workflow

- Branch: `advisor/015-updater-pin`
- Commit message: `fix(deps): align tauri-plugin-updater to 2.13.3 and add plugin pin check`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Pin the Rust updater to 2.13.3

In `crates/teletype-desktop/Cargo.toml`, change:
```
tauri-plugin-updater = "2"
```
to:
```
tauri-plugin-updater = "2.13.3"
```

Then run `cargo update -p tauri-plugin-updater` to update Cargo.lock.

**Verify**: `grep -A1 'name = "tauri-plugin-updater"' Cargo.lock` shows `version = "2.13.3"`.

### Step 2: Add a plugin version consistency check to check-artifact-pins.sh

Add a new section to `scripts/check-artifact-pins.sh` that compares the JS and Rust versions for all five shared Tauri plugins:

| JS package (ui/package-lock.json) | Rust crate (Cargo.lock) |
|---|---|
| `@tauri-apps/api` | `tauri` |
| `@tauri-apps/plugin-dialog` | `tauri-plugin-dialog` |
| `@tauri-apps/plugin-opener` | `tauri-plugin-opener` |
| `@tauri-apps/plugin-process` | `tauri-plugin-process` |
| `@tauri-apps/plugin-updater` | `tauri-plugin-updater` |

For each pair, extract the major.minor.patch version from both lockfiles and compare. If any pair disagrees, print a drift message and exit 1.

Implementation approach (follow the existing script's style):
- Use `grep` + `sed` to extract versions (the script already uses this pattern for llama.cpp).
- For the JS side: `grep -A1 '"@tauri-apps/plugin-updater"' ui/package-lock.json | grep version | sed ...`
- For the Rust side: `grep -A1 'name = "tauri-plugin-updater"' Cargo.lock | grep version | sed ...`
- Compare with `[[ "$js_ver" != "$rust_ver" ]]`.

Add env overrides for the two lockfiles (mirroring the existing pattern):
- `UI_LOCKFILE=...` (default `ui/package-lock.json`)
- `CARGO_LOCKFILE=...` (default `Cargo.lock`)

**Verify**: `bash scripts/check-artifact-pins.sh` → exit 0, all plugin pairs aligned.

### Step 3: Negative test

Temporarily change the JS updater version in a test copy (or use the env override) to `2.13.2`. Run the check → expect exit 1 with a drift message. Revert.

**Verify**: drift detected (exit 1), revert restores (exit 0).

## Test plan

- The pin-consistency script IS the test. Step 3 (negative test) proves it catches drift.
- `cd ui && npx tsc --noEmit` → exit 0 (no type errors from the version bump).
- `cd ui && npm test` → all pass (updater UI tests still green).

## Done criteria

- [ ] `grep -A1 'name = "tauri-plugin-updater"' Cargo.lock` shows `version = "2.13.3"`
- [ ] `bash scripts/check-artifact-pins.sh` exits 0 and reports all plugin pairs aligned
- [ ] `bash scripts/check-artifact-pins.sh` exits 1 when a plugin version is drifted (negative test)
- [ ] `cd ui && npx tsc --noEmit` exits 0
- [ ] `cd ui && npm test` passes
- [ ] No files outside the in-scope list are modified (`git diff --name-only`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- `tauri-plugin-updater 2.13.3` does not exist in the Cargo registry (check `cargo search tauri-plugin-updater`).
- The version bump causes a compile error in `teletype-desktop` (API change between 2.13.0 and 2.13.3).
- A step's verification fails twice after a reasonable fix attempt.

## Maintenance notes

- If a new Tauri plugin is added, add it to the check table in Step 2.
- The check compares major.minor.patch — pre-release suffixes (e.g. `-beta`) will not match and will correctly flag drift.
- This check runs in CI via the existing `checks` job (it calls `check-artifact-pins.sh`).
