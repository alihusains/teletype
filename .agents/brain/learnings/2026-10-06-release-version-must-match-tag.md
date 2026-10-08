---
id: 2026-10-06-release-version-must-match-tag
title: Tauri ships the version from tauri.conf.json, not the git tag
status: verified
type: debugging
area: release / CI plumbing
agent: codex
date: 2026-10-06
---

# Tauri ships the version from tauri.conf.json, not the git tag

## Problem

A `v0.2.4` tag produced `Teletype_0.2.2_aarch64.dmg`. The release pipeline
builds fine, signs, and publishes — but the artifact name (and the version
the app reports) came from `crates/teletype-desktop/tauri.conf.json` and the
workspace `Cargo.toml`, which had drifted two versions behind the tag.

## Symptoms

- Artifact filenames did not match the tag.
- No CI failure: the build, signing, and publish jobs all succeeded.

## Root cause

Tauri bakes the version from `tauri.conf.json` (bundle name/filename) and the
workspace `Cargo.toml` (crate/app version) into the output. The git tag is
only the trigger. Nothing in the pipeline kept the two in sync, so every
manual version bump that touched only one file (or the tag alone) produced a
misnamed release.

## Fix

Every job in `.github/workflows/release.yml` (macOS, Windows, publish) now
runs a "Sync version to the tag" step right after checkout:

```
VER="${GITHUB_REF_NAME#v}"
sed -i '' "s/^version = \".*\"/version = \"$VER\"/" Cargo.toml
python3 -c "... rewrite tauri.conf.json version ..."
```

(with the PowerShell equivalent on the Windows job).

## Verification

Step prints the rewritten values; artifact naming now follows the tag by
construction rather than by coincidence.

## Reusable lesson

When a build system reads its version from a file, the git tag is not the
source of truth — sync the file to the tag in CI, or the tag and the artifact
will drift silently.
