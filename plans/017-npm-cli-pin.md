# Plan 017: Pin @tauri-apps/cli to match cargo tauri-cli 2.12.0

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 8746c1c..HEAD -- ui/package.json ui/package-lock.json .github/workflows/release.yml README.md`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: 015 (shares the pin-check script; run 015 first or apply both in one commit)
- **Category**: dx
- **Planned at**: commit `8746c1c`, 2026-11-26

## Why this matters

A developer running the app locally uses the npm `@tauri-apps/cli` (currently
2.11.4 via caret `^2.5.0`), while the release pipeline uses cargo `tauri-cli`
2.12.0 (pinned in `release.yml` and `README.md`). The repo has already burned
a CI cycle on exactly this class of drift (a comment in `release.yml:802-803`
documents a 2.9.3/2.12.0 mismatch that caused a reinstall every run). The npm
CLI version is not pinned, so a fresh `npm ci` on a different day can silently
move it, while the cargo side stays at 2.12.0.

## Current state

- `ui/package.json:23` — `"@tauri-apps/cli": "^2.5.0"`
- `ui/package-lock.json` — resolves to `2.11.4` (all platform binaries)
- `.github/workflows/release.yml:309-310` — `cargo install tauri-cli --version "2.12.0" --locked`
- `README.md:131` — `cargo install tauri-cli --version "2.12.0" --locked`
- `crates/teletype-desktop/tauri.conf.json:7-9` — `beforeDevCommand`/`beforeBuildCommand` run `npm run dev`/`npm run build` (the npm CLI drives local dev)

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Check available versions | `npm view @tauri-apps/cli versions --json` | lists 2.12.0 or closest |
| Bump npm CLI | `cd ui && npm install @tauri-apps/cli@<version> --save-exact` | package.json + lock updated |
| Typecheck | `cd ui && npx tsc --noEmit` | exit 0 |
| UI tests | `cd ui && npm test` | all pass |

## Scope

**In scope** (the only files you should modify):
- `ui/package.json` (pin `@tauri-apps/cli` exactly)
- `ui/package-lock.json` (updated by npm)

**Out of scope** (do NOT touch):
- `Cargo.toml`, `Cargo.lock` — the cargo side is already pinned
- Any Rust or TypeScript source file
- `release.yml` — the cargo CLI pin is already correct

## Git workflow

- Branch: `advisor/017-npm-cli-pin`
- Commit message: `fix(dx): pin @tauri-apps/cli to match cargo tauri-cli 2.12.0`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Check the available @tauri-apps/cli versions

Run `npm view @tauri-apps/cli versions --json` and look for `2.12.0` or the
closest version to the cargo `tauri-cli` 2.12.0. The npm package and the
cargo crate are versioned in lockstep by the Tauri team, so `2.12.0` should
exist. If it does not, pick the closest available version and note the
discrepancy in your report.

**Verify**: you have identified the exact version to pin.

### Step 2: Pin the npm CLI exactly

In `ui/package.json`, change:
```json
"@tauri-apps/cli": "^2.5.0"
```
to:
```json
"@tauri-apps/cli": "2.12.0"
```
(exact pin, no caret).

Then run `cd ui && npm install @tauri-apps/cli@2.12.0 --save-exact` to update
the lockfile. If `2.12.0` is not available, use the version from Step 1.

**Verify**: `grep "@tauri-apps/cli" ui/package.json` shows the exact pin. `grep -A1 '"@tauri-apps/cli"' ui/package-lock.json` shows the same version.

### Step 3: Verify the UI still builds

**Verify**: `cd ui && npx tsc --noEmit` → exit 0. `cd ui && npm test` → all pass.

## Test plan

- The version pin is the test. `grep` confirms the pin is exact (no caret).
- `cd ui && npx tsc --noEmit` → exit 0 (no type errors).
- `cd ui && npm test` → all pass (npm CLI change does not affect UI code).

## Done criteria

- [ ] `ui/package.json` has `"@tauri-apps/cli": "2.12.0"` (exact, no caret) or the closest available version
- [ ] `ui/package-lock.json` resolves to the same version
- [ ] `cd ui && npx tsc --noEmit` exits 0
- [ ] `cd ui && npm test` passes
- [ ] No files outside the in-scope list are modified (`git diff --name-only`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- `@tauri-apps/cli` version 2.12.0 (or the closest) does not exist in the npm registry.
- The version bump causes a `npm install` failure (peer dependency conflict).
- A step's verification fails twice after a reasonable fix attempt.

## Maintenance notes

- If the cargo `tauri-cli` is bumped (e.g. to 2.13.0), bump the npm `@tauri-apps/cli` in the same commit.
- Plan 015's plugin-pins check could be extended to also compare the npm CLI version against the cargo CLI version — but that requires reading `release.yml` for the cargo version, which is a different extraction pattern. Deferred to a follow-up.
- The `README.md` setup section already tells contributors to install cargo `tauri-cli` 2.12.0 — no README change needed.
