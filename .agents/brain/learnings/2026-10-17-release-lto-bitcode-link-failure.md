# L036 — Release LTO link fails for teletype-eval: whisper.cpp objects carry no `__bitcode`

- **ID:** L036
- **Title:** `cargo build --release -p teletype-eval` dies at the final link with `failed to get bitcode from object file for LTO (Can't find section __bitcode)`; build the dev tool with `--config profile.release.lto=false`
- **Date:** 2026-10-17
- **Agent/tool:** OpenCode lead session (worktree `ahsorathiya7-fuzzy-happiness`, branch `feat/beat-wispr`)
- **Project area:** workspace `[profile.release]` / `crates/parakeet-sys` (whisper.cpp) / `crates/teletype-eval`
- **Status:** verified
- **Evidence:** two independent release builds of `teletype-eval` with the workspace default `lto = true` failed identically (`error: failed to get bitcode from object file for LTO (Can't find section __bitcode)` → `could not compile \`teletype-eval\``), reproduced in both the `feat/beat-wispr` worktree and the `refactor` checkout (same failure class, so it is toolchain/dependency-graph, not branch, specific). `cargo build --release -p teletype-eval --config profile.release.lto=false` finished in 2m10s and produced a working 48 MB binary.

## Problem

Gate P3.8 (measure: engine bake-off) needs the `teletype-eval` release binary,
but the plan's own gate command (`cargo build --release -p teletype-eval`)
fails on this machine at the final link step.

## Root cause

The workspace release profile sets `lto = true` (fat LTO). lld in fat-LTO mode
requires every linked object to embed LLVM bitcode (`__bitcode` section);
objects that are plain machine code abort the link with the error above.
`teletype-eval` is the one crate whose link graph pulls in third-party C
objects compiled by a build script: `parakeet-sys` (whisper.cpp/llama.cpp
C/C++). [INFERENCE — not isolated per-object: the error does not name the
offending file.] The app binaries that never link that object graph are not
affected; only crates that link `parakeet-sys` directly would break, and
`teletype-eval` is the one that does (the eval benchmarks run ggml
transcription in-process).

Secondary trap: cargo's `--config` schema for `profile.release.lto` accepts
only booleans at the CLI (`lto=thin` → `invalid boolean, expected \`true\``).
The `fat`/`thin` strings are manifest-TOML-only. So the CLI override has to be
`lto=false` (never `lto=thin`); thin-LTO behavior was therefore never
reachable from the CLI and was not tested through it.

## Rule

- Build `teletype-eval` (and any future crate that links `parakeet-sys`
  release) with `--config profile.release.lto=false`. The workspace manifest
  keeps `lto = true` for the shipping profile; do not flip it globally for a
  dev tool.
- `scripts/eval/run_bakeoff.sh` now self-heals: if no release binary exists it
  builds with the flag (2026-10-17).
- A stale `target/release/teletype-eval` from an older source tree breaks the
  bake-off in a different way (missing `--boost` subcommand, exit 2): after
  code moves, delete or rebuild the tool, don't trust the timestamp.
- The LTO link failure prints a bare `error: failed to get bitcode...` line,
  NOT a compiler `error[E0xxx]` — pipelines that grep for `error\[` report
  "no errors" while the build failed (bit this twice on 17 Oct: a
  `cargo build ... | grep "error\["` came back empty and the STALE binary was
  quietly used, producing wrong output). After any release build of this
  crate, check the cargo output or the binary's mtime/behavior, never
  assumption.
- `CARGO_PROFILE_RELEASE_LTO=false cargo build --release` is the env-var
  equivalent of the `--config` form; verified 17 Oct (24 s incremental build,
  working binary). Equivalent to the `run_bakeoff.sh` recipe.

## Verification

2026-10-17: `cargo build --release -p teletype-eval --config
profile.release.lto=false` → `Finished release profile in 2m10s`; binary runs
(`--help` incl. `--boost`); the same build without the flag fails at link as
documented above.
