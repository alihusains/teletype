---
id: 2026-10-03-t Clearing the updater key is not skipping updater artifacts
title: Clearing TAURI_SIGNING_PRIVATE_KEY does not stop Tauri building the updater payload
status: verified
type: debugging
area: release / signing
agent: kilo
tool: kilo-auto/free
date: 2026-10-03
---

# Clearing `TAURI_SIGNING_PRIVATE_KEY` does not stop Tauri building the updater payload

## Problem

`release.yml` claims in its own header that "Without the Apple credentials the
workflow still publishes an unsigned build". Five release attempts (v0.2.0
twice, v0.2.1 twice, v0.2.2 once) all failed anyway. After the Apple signing
and artifact-path problems were fixed, v0.2.2 got all the way through
compilation and bundling and then died on its last line.

## Symptoms

    Finished 2 bundles at:
        .../Teletype.app (39.28 MiB)
        .../Teletype_0.2.2_aarch64.dmg (20.20 MiB)
    Bundling .../Teletype.app.tar.gz (updater) (19.59 MiB)
    Error A public key has been found, but no private key. Make sure to set
    `TAURI_SIGNING_PRIVATE_KEY` environment variable.
    ##[error]Process completed with exit code 1.

All three build jobs (macOS arm64, macOS Intel, Windows NSIS) failed
identically. The Intel job's failure was the same message 11 minutes later.

## Initial hypotheses

1. The minisign key secret is malformed (that was the *previous* failure, in
   v0.2.0/v0.2.1, and it was real then).
2. Apple codesign is still being attempted.
3. **The updater artifact is a separate gate from Apple signing, and clearing
   the key env does not disable it** (confirmed).

## Investigation

1. Read the full macOS job log rather than the last line: the app and the DMG
   were both built successfully. Nothing about Apple signing failed - there is
   no certificate, so Tauri never attempted codesign.
2. `tauri.conf.json` sets `bundle.createUpdaterArtifacts: true` (nested under
   `bundle`, not at the root - a top-level `get` misses it).
3. `plugins.updater.pubkey` is set. Tauri reads the pubkey, builds the
   `.app.tar.gz`, then looks for the matching private key to produce the
   `.sig`, finds none, and aborts.
4. The existing empty-secret handling unset `TAURI_SIGNING_PRIVATE_KEY` and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. That is necessary and not sufficient:
   removing the key does not tell Tauri to stop building an artifact that
   cannot be signed.

## Evidence

- Release run 37151311285, all three build jobs: identical
  `A public key has been found, but no private key`.
- `crates/teletype-desktop/tauri.conf.json`: `bundle.createUpdaterArtifacts: true`
  and `plugins.updater.pubkey` present.
- The DMG and `.app` were complete on disk at the moment of failure, so
  nothing about the Apple half of the pipeline was broken.
- `cargo tauri build --help`: `-c, --config <CONFIG>` - "JSON strings or paths
  to JSON, JSON5 or TOML files to merge with the default configuration file",
  where a later value overwrites an earlier conflicting one.

## Root cause

Two independent signing gates exist in a Tauri release:

1. **Apple codesign / notarization**, driven by `APPLE_CERTIFICATE` et al.
2. **Updater payload signing**, driven by `TAURI_SIGNING_PRIVATE_KEY`, and
   demanded whenever `bundle.createUpdaterArtifacts` is true and an updater
   `pubkey` exists.

The workflow only knew about gate 1. It degraded gate 1 correctly and had no
lever on gate 2, so the "unsigned build" path it advertised could never work.

## Fix

All three build jobs now branch on the key and, when there is none, pass

    cargo tauri build ... --config '{"bundle":{"createUpdaterArtifacts":false}}'

Done at the CLI, **not** in `tauri.conf.json`. Editing the committed config
would make updater payloads impossible for every future release, including
properly signed ones. `--config` merges over the file, so a release with a
key still produces payloads and `.sig` files unchanged.

## Verification

- Release run 37153134807: all four jobs succeeded.
- macOS arm64 passed `Build Tauri app`, `Record the architecture` (the
  `test -f "$BIN"` against `$CARGO_TARGET_DIR`) and `upload-artifact`.
- v0.2.2 published with `Teletype_0.2.2_aarch64.dmg` (20 MiB),
  `Teletype_0.2.2_x64-setup.exe` (17 MiB) and `latest.json`.
- `latest.json` is `{"version":"0.2.2","platforms":{}}` - empty platforms, so
  the updater advertises nothing. That is the designed behaviour for an
  unsigned release: the publish job would rather disable updates than offer a
  download it cannot verify.
- Workflow parses; the JSON passes through bash and PowerShell single quotes
  unparsed.

## Known gaps this release still has

Found while verifying, not fixed, because each needs another ~30 min release
cycle and a decision:

1. **The `.app` never ships.** `release.yml` lists `dist/**/*.app` in the
   release files, but a macOS `.app` is a *directory*, and a GitHub release
   asset must be a file. The glob silently matches nothing. The arm64 `.app`
   is 39 MiB in the build artifact and absent from the release.
2. **Intel macOS users get nothing.** The Intel job runs
   `--bundles app` only, so its sole product is the `.app` directory from
   gap 1. Its 20 MiB artifact contributed zero release assets. The workflow
   header advertises Intel as "an option we offer", and this release offers
   nothing.
3. **In-app updates are disabled** until a signing key exists, since the
   manifest has no platforms.

## Reusable lesson

- **Tauri has two separate signing gates.** Apple codesign/notarization and
  updater-payload signing are independent, driven by different variables. A
  workflow that only understands one of them will fail at the other's step,
  after a full compile. Enumerate both before claiming an unsigned path works.
- **Removing a credential does not disable the feature that needs it.** The
  empty-secret handling unset the key, which read like "skip signing" but only
  removed the input. Features that *require* a credential must be switched off
  explicitly.
- **Override generated config at the CLI, not in the committed file.** `--config`
  merges inline JSON over `tauri.conf.json`, which is how you make a signed
  release and an unsigned release differ without two config files or a config
  edit that breaks the signed path forever.
- **Read the whole log, not the last line.** "A public key has been found, but
  no private key" reads like a secrets problem. Two lines earlier it had
  already finished a successful 39 MiB `.app` and a 20 MiB DMG. The message
  was about the *next* artifact, not the build.
- **An empty `platforms` in an updater manifest is a feature, not a bug.** It
  is what stops the updater offering a download the app cannot verify.

## Sources

- Release runs: 37151311285 (failed on the updater gate), 37153134807
  (published v0.2.2)
- Fix commit: `499489a` on `main`