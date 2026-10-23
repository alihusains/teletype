# L004: A 0-byte bundle-resource stub shadowed the llama-server lookup and surfaced as "Permission denied"

**ID:** L004
**Date:** 2026-09-25
**Agent:** OpenCode
**Project area:** teletype-inference / build scripts / model activation
**Status:** verified
**Evidence:** user screenshot "Download failed: failed to spawn /Users/a.sorathiya/Documents/Ali/teletype/target/debug/llama-server: Permission denied (os error 13)"; `stat` showed 0 bytes / `-rw-r--r--` on both stubs; fix verified by 272 passing tests, a self-contained `llama-server`, and a live EG-1 `/health` 200 + chat completion using the exact `warm_up` args.

## Problem

Selecting the EG-1 model in the Models screen failed. The error blamed the download
path, but the failure was in spawning the local LLM runtime.

## Symptoms

- UI error: `Download failed: failed to spawn .../target/debug/llama-server: Permission denied (os error 13)`.
- EG-1 shards were already on disk (2.7 GB in `models/eg-1/`), so no download was needed.
- `stat` reported `target/debug/llama-server` and
  `crates/teletype-desktop/binaries/llama-server` as **0 bytes, mode 644** (created
  2026-09-22 / 2026-09-25).
- No `llama-server` build had ever been run on this machine
  (`/tmp/llama.cpp-teletype` absent).

## Initial hypotheses

1. The model download itself failed (wrong: shards were complete; the error came from
   `select_model` → `warm_up`, which both write to the same `downloadError` state in
   `ModelsScreen.tsx`).
2. macOS TCC / sandbox denial (wrong: `EACCES` came from `Command::spawn`, not from the OS
   security layer).
3. The binary was a placeholder that could not execute.

## Investigation

1. Traced `Download failed:` to `ModelsScreen.tsx` (`download` and `selectLocal` both call
   `setDownloadError`) and `failed to spawn` to a single site:
   `crates/teletype-inference/src/server.rs:105` inside `ServerProvider::warm_up`.
2. `find_llama_server()` resolves candidates with `Path::is_file()` only, so the first
   match next to the exe won regardless of size or exec bit.
3. Checked `.gitignore` and `tauri.conf.json`: `bundle.resources` declares
   `binaries/llama-server`, so a stub must exist for `cargo tauri build` to succeed -
   which is why empty files were sitting there.
4. After installing a real binary, `otool -l` revealed a second, latent failure: the
   default shared build had `LC_RPATH=/tmp/llama.cpp-teletype/build-teletype/bin` and
   ~10 sibling dylibs, while the bundle ships only the one executable.

## Evidence

- `find / -name llama-server`-style checks: both copies 0 bytes; nothing in the repo
  creates them (grep for `binaries/llama-server` only hit `tauri.conf.json` and the brain).
- `otool -L` before the fix: `@rpath/libllama-server-impl.dylib`, `libllama-common`,
  `libmtmd`, `libllama`, `libggml*` plus `/opt/homebrew/opt/openssl@3/lib/libssl.3.dylib`.
- `otool -L` after the fix: only `/usr/lib` and `/System/Library` entries; `LC_RPATH` count 0.

## Root cause

Two stacked defects:

1. **Lookup accepted anything that `is_file()`**, so a 0-byte, non-executable stub left
   behind for the bundle resource was selected as the runtime and `spawn()` returned
   `EACCES` (os error 13). The message is misleading because `EACCES` reads as a
   permissions/sandbox problem, not "this file is empty".
2. **The build script produced a non-relocatable binary**: shared ggml/llama dylibs
   resolved through an absolute `/tmp` rpath, and it linked Homebrew OpenSSL. Even with
   defect 1 fixed, the packaged `.app` could not run it.

## Fix

- `crates/teletype-inference/src/server.rs`: added `is_runnable_binary()` (non-empty,
  `is_file`, exec bit on unix) used by the env override, the exe-dir candidates, and
  `find_in_path`; the not-found error now surfaces instead of `EACCES`. New test
  `placeholder_stubs_are_not_runnable`.
- `scripts/build-llama-server.sh`: added `-DBUILD_SHARED_LIBS=OFF` (single self-contained
  executable, no `/tmp` rpath, no sibling dylibs to bundle) and `-DLLAMA_OPENSSL=OFF`
  (Teletype only talks to this server over `127.0.0.1` HTTP and does its own model
  downloads, so its HTTPS support is unused and unbundleable).

## Verification

- `cargo test --workspace`: 272 passed / 0 failed (271 baseline + 1 new).
- `cargo clippy --workspace --all-targets`: no warnings in the changed file.
- `llama-server --version` succeeds with the llama.cpp build dir **moved away**, proving
  no hidden rpath dependency; `otool -L` shows zero non-system dylibs.
- Exact `warm_up` arg set against the real EG-1 entrypoint shard: `/health` → 200 in
  ~4 s, and `POST /v1/chat/completions` returned a corrected sentence. Server killed
  afterwards, no orphans (Gotcha G008).
- App restarted via `cargo tauri dev`: starts clean, Parakeet loads on Metal, a live
  dictation transcribed in 167 ms.

## Reusable lesson

When an error names an executable, `stat` it before debugging anything else: size, mode,
and whether it is a real binary (`file`). Bundle/resource declarations that must exist at
packaging time tend to accumulate 0-byte placeholders that later get *executed*. Also
treat "Permission denied" from `Command::spawn` as "this file is not a runnable program"
first and a TCC/sandbox problem second.

Any binary shipped as a single bundled resource must be self-contained: check
`otool -L` (macOS) / `ldd` (Linux) for non-system deps and an absolute `LC_RPATH`, and
test it with the build directory removed.

## Evidence / sources

- Commit: pending (uncommitted working-tree changes)
- Documentation: `scripts/build-llama-server.sh`, `crates/teletype-inference/src/server.rs`
- User confirmation: screenshot of the Models screen error (2026-09-25)

---
