---
id: 2026-10-04-empty-release-glob-is-not-an-error
title: A release glob that matches nothing is silently a success, and `.app` is a directory
status: verified
type: debugging
area: release / packaging
agent: kilo
tool: kilo-auto/free
date: 2026-10-04
---

# A release glob that matches nothing is silently a success, and `.app` is a directory

## Problem

v0.2.2 published successfully. It shipped two assets:

- `Teletype_0.2.2_aarch64.dmg`
- `Teletype_0.2.2_x64-setup.exe`

The workflow's publish step also asked for `dist/**/*.app`, and the build
artifact for macOS did contain a `Teletype.app`. No `.app` asset appeared on
the release page, and no job went red. A 39 MiB app bundle was built, cached,
downloaded into the publish job, and then dropped.

Separately, the Intel (`x86_64-apple-darwin`) job ran
`cargo tauri build --bundles app` and uploaded only `bundle/macos/*`. Its
upload glob listed no `bundle/dmg/*` path, and `app` does not produce a DMG,
so Intel contributed **zero** publishable assets to a release that claims to
support Intel macOS.

## Symptoms

- A release looks complete and green, but is missing an entire artifact class.
- No warning from `softprops/action-gh-release`.
- No failure from `actions/upload-artifact`, because the artifact *was* uploaded
  (with the `.app` directory in it), just not as a release asset.
- `gh release view <tag> --json assets` is the only place the absence shows up.

## Initial hypotheses

1. `action-gh-release` skipped the directory silently because of a path typo.
2. The `.app` was excluded by an `if:` on the publish step.
3. `upload-artifact` dropped it, since `.app` is a directory tree.
4. A `.gitignore` or `.gitattributes` rule stripped it.

## Investigation

The path in `release.yml` matched a real file in the artifact. The publish
step had no conditional. `upload-artifact@v4` had no complaint, because its
`path:` list also included `ARCH`, `*.app.tar.gz` and `*.dmg`, and it is
configured `if-no-files-found: error` — that check only fires when the *whole*
set is empty, never when one line of the set matches nothing.

The decisive detail: **a GitHub release asset must be a file, and a macOS
`.app` is a directory.** `@actions/glob` does match `dist/**/*.app` against a
directory, but the upload then has nothing file-shaped to attach, and
`action-gh-release` treats "this glob produced no files" as success rather than
error. A directory can never satisfy the requirement.

## Evidence

`gh release view v0.2.2 --json assets` lists exactly two assets. Neither is an
`.app` or a `.app.zip`. The Intel job's upload block, read directly, has no
`bundle/dmg/*` line and its build command passes `--bundles app`.

A local simulation of the proposed replacement confirmed `find ... -type d`
picks only the bundle directory and never the `.zip` written next to it, and
that `ditto -c -k --sequesterRsrc --keepParent` preserves both the executable
bit on `Contents/MacOS/<bin>` and the symlinks under `Contents/Frameworks`.

## Root cause

Two separate packaging mistakes with the same theme — a step whose *shape* looks
correct while its *output* is never asserted:

1. `dist/**/*.app` in the publish `files:` list could never produce an asset,
   because a `.app` is a directory. Nothing failed when it produced nothing.
2. The Intel job built a bundle type (`app`) that has no DMG equivalent and
   uploaded no DMG path, so it had nothing publishable at all.

## Fix

- Both macOS jobs zip the bundle with
  `ditto -c -k --sequesterRsrc --keepParent` (the same invocation Tauri's own
  `bundle_dmg.sh` uses) and upload `*.app.zip`.
- Intel builds `--bundles app,dmg` and uploads `bundle/dmg/*.dmg`. The DMG is
  renamed `Teletype-x86_64.dmg` rather than trusting the bundler's arch suffix,
  because the two macOS DMGs share one release and GitHub keys assets by name.
- The publish job gained a pre-upload gate that asserts a `.app.zip`, a `.dmg`
  and a `.exe` actually reached `dist/`. This is the check whose absence let
  the bug ship.

## Verification

Not yet run end to end. YAML parses, job graph is unchanged, `release-macos`
and `release-windows` remain required while `release-macos-intel` stays
`continue-on-error: true`, and the new shell logic was exercised locally
against a synthetic bundle and a synthetic nested `dist/` tree. The fix ships
unproven until the next real tag. The gate added in this change is what makes
that next tag self-checking.

## Reusable lesson

**Assert on artifacts, never on globs.** A `files:` list, an `upload-artifact`
path, or a `find` that matches nothing is not an error in any of these
actions — it is a silent success. The only reliable check is to count real
files by name after download and fail if the count is zero.

Three specific traps this generalises to:

- **Directory vs file.** `*.app`, `*.app.tar.gz` as a directory, and any bundle
  format that is itself a folder cannot be a release asset. Zip it first.
- **`if-no-files-found: error` is all-or-nothing.** It guards the whole `path:`
  list, not each line. One stray glob hides forever inside a list where
  everything else matched.
- **Bash does not glob `**`.** Without `shopt -s globstar`, `dist/**/*.dmg`
  means `dist/*/*.dmg` and will not match `dist/macos-release/dmg/foo.dmg`.
  `@actions/glob` (inside the actions) does understand `**`. Use `find` for
  assertions in shell steps.

Corollary: verify a release by listing its assets, not by reading the workflow
green. `gh release view <tag> --json assets` is the ground truth.