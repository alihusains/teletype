# L097 — A Sentry panic hook replaces yours, so chain instead of re-registering

- **ID:** L097
- **Title:** A Sentry panic hook replaces yours, so chain instead of re-registering
- **Date:** 2026-11-22
- **Agent:** Buffy (Freebuff), Sentry agent handoff
- **Area:** desktop / telemetry / panic reporting
- **Status:** verified

## Problem

Adding Sentry to Teletype had to keep the existing panic behaviour: `run()`
installs a hook that writes a local `crash-*.json` for the next-launch banner
(`crash.rs`). A remote report without the local one loses the offline case; the
local one without the remote leaves field crashes invisible, which is the whole
reason the SDK was added.

## Symptoms / initial hypotheses

1. *Hypothesis:* `set_hook` after `sentry::init` reports both. **Wrong** — it
   replaces whatever hook `init` installed, so panics reached the local report
   and never Sentry.
2. After chaining, one smoke panic produced **three** Sentry issues:
   `panic: Sentry panic smoke test …`, `thread: main`, and the breadcrumb-only
   line. One crash, three issues.

## Investigation

- `sentry-rust`'s `panic` integration installs its hook in `ClientOptions` setup,
  i.e. inside `sentry::init`. `std::panic::take_hook()` after `init` therefore
  returns *Sentry's* hook.
- The three issues came from the local hook logging `PANIC: …`, `  at …` and
  `  thread: …` through `log_entry`, which the new error-log bridge maps to one
  event per error-level line.

## Root cause

The panic hook is a single slot, and the local hook described one panic in three
log lines that the error-log bridge could not tell apart from three failures.

## Fix

- `let previous = std::panic::take_hook();` after `telemetry::init()`, then
  `set_hook` once and end the closure with `previous(info)`; the local report is
  written first so it survives a dead network.
- `telemetry::mute_error_events()` at the top of the hook: the hook's own lines
  stay breadcrumbs on the SDK's panic event instead of becoming issues.

## Verification

Built binaries, both paths, checked in the Sentry project `duas-and-aamal/teletype`:

| Trigger | Result |
|---|---|
| error-level log line from the release binary (`TELETYPE_SENTRY_SMOKE`, temporary) | `TELETYPE-1`, level error, release `teletype@0.2.21`, OS `macOS 26.6.2`, device `Mac14,6` |
| panic in the debug binary (`TELETYPE_SENTRY_PANIC_SMOKE`, temporary) | `TELETYPE-3` only, one issue, panic mechanism, breadcrumbs `PANIC: …` / `  at …` / `  thread: main` |

Both temporary triggers were removed afterwards; `cargo clippy -D warnings`,
643 desktop lib tests and 610 vitest tests pass without them.

## Reusable lesson

Treat `set_hook` as a single slot: capture the previous hook and call it, and
never let a diagnostic fan-out (one panic → N log lines) reach an event-per-line
bridge. A smoke test proves nothing unless it runs the shipped binary's real
`init`; and a client initialised at `run()` cannot report a panic before the
consent flag has been read from settings.

## Caveats

The handoff token can add and resolve but not delete issues, so the three smoke
issues stay in the project until someone removes them in the UI. Production
stack traces are readable only after `release.yml` uploads dSYMs
(`SENTRY_AUTH_TOKEN`), which is not wired yet.
