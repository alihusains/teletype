# Release v0.2.21

Status: published as an actual (non-prerelease) GitHub release.

- Release: https://github.com/alihusains/teletype/releases/tag/v0.2.21
- Tag: `v0.2.21`
- Commit: `75a8ad3`
- Latest release on GitHub: v0.2.21

## What shipped

- Full v0.2.20 feature set: polish profiles, trust layer, visible polish, personalization v2, model defaults, model mirror, hotkey fixes.
- v0.2.21 patch: deterministic backtrack now retracts quantity phrases as one fact and drops duplicated leading function words in replacements.
- Model mirror published at `alihusains/teletype-models@models-v1` with S1-mini, SpeakoFlow Mini, and Qwen3.5 0.8B/2B, each hash-verified and uploaded from pinned upstream URLs.
- Windows Parakeet Ultra sherpa INT8 remains an official GitHub-hosted artifact; macOS Parakeet Ultra CoreML remains FluidAudio-managed.

## QA

- `cargo test --workspace`: green (core 1396, desktop 631, inference 133 passed/2 ignored, speech 81 passed/1 ignored).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo fmt --all -- --check`: clean.
- Polish eval self-test: PASS.
- `tsc --noEmit`: clean; vitest 58 files / 610 tests.
- CI on tagged commit: success.
- Release workflow: success; manually promoted to latest because no updater signing key is configured, so CI intentionally left it as prerelease.

## Release assets

`latest.json`, `Teletype-x86_64.app.zip`, `Teletype-x86_64.dmg`, `Teletype.app.zip`, `Teletype_0.2.21_aarch64.dmg`, `Teletype_0.2.21_x64-setup.exe`.

## Caveat

No Apple signing/notarization or updater signing key is configured in CI. The release is published and marked latest as requested, but `latest.json` has zero platforms, so in-app auto-update is effectively disabled until signing secrets are added.
