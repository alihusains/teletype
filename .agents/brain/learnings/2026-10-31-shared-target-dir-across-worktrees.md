# Shared CARGO_TARGET_DIR across worktrees links the wrong workspace crates

- ID: L-2026-10-31-shared-target
- Date: 2026-10-31
- Agent: Claude Code (dictation-detail worktree)
- Area: build / verification
- Status: verified (reproduced twice on 2026-10-31)

## Problem

Several worktrees (`teletype-wt/dictation-detail`, `teletype-wt/pill-fixes`, ...)
build with the same `CARGO_TARGET_DIR=teletype-wt/integration/target` to save disk.

## Symptoms

`cargo clippy` in one worktree failed with "no field `recording_path` on
`DictationEntry`", and the error pointed at
`teletype-wt/pill-fixes/crates/teletype-core/src/persist.rs`, a file in a
different worktree.

## Root cause

Cargo hashes a path dependency relative to the workspace root, so the same
workspace crate in two worktrees gets the same artifact name. Whichever
worktree built last wins, and its dep-info lists its own absolute paths, so the
other worktree sees the artifact as fresh and links against the wrong source.
This can produce a false failure or, worse, a false pass.

## Fix / workaround

Before trusting a result in a shared target dir: wait until no other `cargo`
runs, `cargo clean -p` every workspace member (`teletype-core`,
`teletype-speech`, `teletype-fluid`, `teletype-inference`, `teletype-desktop`,
`teletype-decider`), then run fmt, clippy and test in one go. Check that the
test output names tests only your branch has.

## Second reproduction (same day)

`cargo clean -p` on every workspace member was not enough on its own: another
agent's `cargo test -p teletype-core -p teletype-desktop` was running in
`pill-fixes` at the same time, rebuilt `teletype-core` into the shared dir
between this worktree's core and desktop builds, and the desktop build failed
with 54 "no field" errors pointing at `pill-fixes/crates/teletype-core`. Core
tests and clippy had passed seconds earlier. Waiting until
`pgrep -f "cargo (test|build|clippy|check)"` was empty, then cleaning and
running clippy plus tests in one command, gave a clean result.

## Reusable lesson

A shared target dir saves disk but makes workspace-crate results untrustworthy
when worktrees build concurrently. Clean your own workspace packages first.
