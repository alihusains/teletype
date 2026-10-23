---
id: 2026-10-03-release-artifact-paths-wrong
title: Every artifact path in release.yml pointed at a directory that has never existed
status: verified
type: debugging
area: release / CI plumbing
agent: kilo
tool: kilo-auto/free
date: 2026-10-03
---

# Every artifact path in release.yml pointed at a directory that has never existed

## Problem

`.github/workflows/release.yml` had four consecutive failed tag runs (v0.2.0
twice, v0.2.1 twice). All three build jobs failed in signing, and the `publish`
job was `skipped` every time, so it had **never executed once**.

Behind those failures sat a second, latent bug: all 22 path references to build
products used a literal `crates/teletype-desktop/target/...`. That directory
does not exist and never has.

## Symptoms

- v0.2.0 and v0.2.1 both died in signing:
  - macOS arm64 + Intel: `failed codesign application: failed to run command
    security import: failed to import keychain certificate`
  - Windows: `failed to decode secret key: incorrect updater private key
    password: Missing comment in secret key`
- `publish` was `skipped` in all four runs.
- The `Report signing state`, `Record the architecture`,
  `Give the Intel assets distinct names` and all three `upload-artifact` steps
  come *after* the build step, so none of them had ever run.

## Initial hypotheses

1. Signing secrets are simply missing or malformed (true, and the cause of the
   four observed failures).
2. Nothing else. Fixing the secrets was assumed to be sufficient.

Hypothesis 2 was never tested, because every run died before reaching the
steps that would have exposed it.

## Investigation

1. Listed the failing steps per run via `gh run view --log-failed`. Every
   failure is inside `Build Tauri app` / `Build Tauri app (NSIS)`, i.e. bundling.
   No step after bundling ever appears in any run.
2. `grep -n "crates/teletype-desktop/target" .github/workflows/release.yml`
   -> 22 hits across all three build jobs.
3. Checked the workspace layout:

       [workspace]
       members = ["crates/parakeet-sys", "crates/teletype-core",
                  "crates/teletype-speech", "crates/teletype-inference",
                  "crates/teletype-desktop", "crates/teletype-eval"]

   `crates/teletype-desktop/Cargo.toml` has no `[workspace]` table, so it is a
   member of the root workspace and **shares the root `target/`**. It has no
   target directory of its own.
4. Found the second compounding factor: all three build jobs set

       CARGO_TARGET_DIR: ${{ github.workspace }}/.cargo-target-cache

   and Tauri honours `CARGO_TARGET_DIR`. The v0.2.1 runner log states the real
   output location outright:

       Bundling Teletype.app (…/.cargo-target-cache/release/bundle/macos/Teletype.app)

   plus `…/.cargo-target-cache/release/teletype` for the binary.

So the literals were wrong twice over: wrong directory (per-crate vs workspace
root) *and* wrong location within it (default `target/` vs
`.cargo-target-cache`).

5. Noticed the Windows job had `CARGO_TARGET_DIR` **unset** while still
   caching `.cargo-target-cache`: cargo built into the default `target/` and
   the cache step saved a directory nothing ever read.

## Evidence

- Run 37130633009 (v0.2.1) macOS job: `Bundling Teletype.app
  (/Users/runner/work/teletype/teletype/.cargo-target-cache/release/bundle/macos/Teletype.app)`.
- `crates/teletype-desktop/target` does not exist locally either, with no
  `CARGO_TARGET_DIR` override, because the crate is a workspace member.
- `publish` shows `skipped` in runs 37004486957, 37007876747, 37129738901,
  37130633009.

## Root cause

Two independent path errors stacked, both invisible until the earlier one was
fixed:

1. `crates/teletype-desktop/target/` - a per-crate path for a crate that is a
   workspace member and therefore uses the root `target/`.
2. `CARGO_TARGET_DIR` is set to `.cargo-target-cache` in every build job, and
   Tauri respects it, so the real prefix is `$CARGO_TARGET_DIR/release/`.

A third, quieter problem: the Windows job cached a cargo target directory it
never pointed cargo at, so ~30 minutes of compilation was thrown away on every
run.

## Fix

Every artifact path now derives from `CARGO_TARGET_DIR`:

- shell steps use `"$CARGO_TARGET_DIR"/release/...`;
- `upload-artifact` uses `${{ env.CARGO_TARGET_DIR }}/release/...`;
- `release-windows` gained the `CARGO_TARGET_DIR` env var it was missing.

This is correct for today's cache dir and stays correct if that value changes.

## Verification

- Workflow parses; all three build jobs resolve `CARGO_TARGET_DIR`, and
  `publish` correctly does not need it.
- Each derived path checked against the real v0.2.1 runner log:
  `$CARGO_TARGET_DIR/release/bundle/macos/*.app` and
  `$CARGO_TARGET_DIR/release/teletype` both match the observed paths.
- **Not** verified by an actual release run at the time of writing. The next
  tag push is the real test, and it is the first time `publish` will ever run.

## Reusable lesson

- **In a Cargo workspace, a member crate has no `target/` of its own.** Any
  script or workflow path of the form `<member>/target/...` is wrong unless
  that crate declares its own `[workspace]`. Use `cargo metadata
  --format-version 1 | jq .target_directory` to get the truth.
- **`CARGO_TARGET_DIR` moves the bundle, not just the intermediate objects.**
  `cargo tauri build` puts its output under the cargo target directory, so
  setting `CARGO_TARGET_DIR` silently relocates `bundle/`. Hardcoded
  `target/release/bundle/...` paths break the moment it is set.
- **Set `CARGO_TARGET_DIR` wherever you cache a target directory.** Caching a
  path cargo does not use is worse than not caching: it costs upload time and
  reads as working.
- **Derive paths from the env var, never from a literal**, in CI steps that
  read build products. One source of truth, and a changed cache dir cannot
  desynchronise the upload step from the build step.
- **A `needs:`-gated job that has always been skipped is unreviewed code.**
  `publish` had never executed across four release attempts; assume any step
  gated behind an always-failing job is unproven, and read it against the real
  artifacts rather than trusting that it "looked fine".
- **Grep the runner log for the real output path** rather than reasoning about
  where a tool should write. The `Bundling Teletype.app (...)` line settled in
  one grep what several paragraphs of reasoning had not.

## Sources

- Release runs: 37004486957, 37007876747, 37129738901, 37130633009
- Fix commit: `6dfa6f4` on `main`
