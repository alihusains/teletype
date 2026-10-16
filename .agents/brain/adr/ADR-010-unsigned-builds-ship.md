# ADR-010 — Ship unsigned builds instead of failing on missing signing secrets

**Date:** 2026-10-03
**Status:** accepted (implemented; `release.yml`, `scripts/check-updater-manifest.sh`, commit `ffca250`)

## Decision

An empty-but-present signing secret (absent or whitespace-only) is treated
as absent. Preflight and build steps trim before deciding whether to sign,
and clear the signing env vars before `cargo tauri build` so Tauri takes its
unsigned path. The updater manifest omits unsigned platforms (with a notice)
instead of refusing to publish; a fully unsigned release writes an
empty-platforms manifest. `pub_date` is stamped in UTC rather than deleted,
because the updater plugin treats a missing `pub_date` as "no update
available."

## Context

The Apple and updater secrets are not yet configured, so every tagged
release was blocked even though an unsigned build installs fine (Gatekeeper
asks the user to right-click open). An empty-but-present secret was worse
than an absent one: Tauri does not distinguish them, and an empty value
fails `security import` / minisign decode deep into a 20–30 minute build.

## Alternatives considered

- Fail the release when signing is absent — rejected: blocks a valid,
  installable artifact.
- Distinguish absent vs. empty at the Tauri layer — rejected: not
  controllable from the workflow side.

## Consequences

Unsigned releases go out with a notice; the updater manifest simply omits
the platforms it cannot advertise. When secrets are configured, the same
code path signs normally with no further change.

## Evidence / source

`PROJECT_BRAIN.md` Decision D010; `release.yml`;
`scripts/check-updater-manifest.sh`; commit `ffca250`.

*(This file was missing while `PROJECT_BRAIN.md` already pointed at it;
reconstructed from the D010 entry on 2026-10-16, content unchanged.)*
