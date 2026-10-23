# v0.2.7 Release: three independent blockers, each a copy of a fact that lived elsewhere

**ID:** L048
**Date:** 2026-10-23
**Agent:** Qoder (release triage)
**Area:** .github/workflows/release.yml, crates/parakeet-cpp-sys/build.rs, CI cache
**Status:** verified for the URL, Intel and smoke fixes; Windows fix awaits a CI run

## Problem

The v0.2.7 Release workflow failed on all three jobs (`gh run view 39296620665`:
macos, macos-intel and windows all `failure`, publish `skipped`). The tag exists,
`gh release list` still shows v0.2.6 as latest, so no v0.2.7 release ever shipped.
Each job failed for a different reason, and none of the three was visible from a
green local `cargo test`.

## 1. macOS smoke: model URL pointed at the wrong Hugging Face repo

Symptom: `curl: (56) The requested URL returned error: 404` followed by exit 56,
about 15 minutes into the job.

Cause: the smoke staged the ggml Parakeet model from
`https://huggingface.co/ggerganov/whisper.cpp/resolve/main/...`, but that repo
only hosts whisper.cpp models. The speech catalog's `PARAKEET_BASE_URL` is
`https://huggingface.co/ggml-org/parakeet-GGUF/resolve/main` (`catalog.rs:19`).
The workflow was a second hardcoded copy of the URL and had drifted.

Second, hidden trap behind the first: the smoke did not set
`TELETYPE_SELF_TEST_MODEL`. Without it the self-test follows the platform
default, which on Apple Silicon resolves to `parakeet-ultra-coreml` (a Fluid
Core ML row), and `self_test.rs` treats a staged dir without that model as
missing (exit 2). So fixing the URL alone would have failed the next run.

Fix: correct URL + `export TELETYPE_SELF_TEST_MODEL=parakeet-tdt-v3`.

Verification: `teletype --self-test` with the pin and the locally installed
model printed `{"ok":true,"transcript":"Hello.","asr_ms":1834,"pipeline_ms":40}`
and exited 0.

## 2. macOS Intel: clang++ linked for the host arch, not the target

Symptom: `ld: warning: ignoring file ...libparakeet_cpp_merged.a(x.o): found
architecture 'x86_64', required architecture 'arm64'` for every object, then
`parakeet-cpp-sys: shared library missing parakeet_capi symbols (16720 bytes)`.

Cause: the Intel job cross-compiles `x86_64-apple-darwin` on an Apple Silicon
runner. CMake produced x86_64 objects (correct), but the dylib link step in
`parakeet-cpp-sys/build.rs` invoked plain `clang++ -shared`, and clang defaults
to the host arch. Every object was skipped, so the dylib was a 16 KB arm64
stub with no C API.

Fix: pass `-arch` from `CARGO_CFG_TARGET_ARCH` (`aarch64` -> `arm64`,
`x86_64` -> `x86_64`) and add the env to `rerun-if-env-changed`.

Verification: `cargo build -p parakeet-cpp-sys --target x86_64-apple-darwin`
finishes, `file` says `Mach-O 64-bit ... x86_64`, and
`nm -gU` shows `_parakeet_capi_load`.

## 3. Windows: cargo cache kept objects built by the previous MSVC

Symptom: `LNK1143: invalid or corrupt file: no symbol for COMDAT section 0x5`
in `libneedle_sys.rlib(needle.cpp.obj)`, after 25 warnings, during the NSIS
build.

Cause: `needle-sys` compiles C++ through CMake in its build script. The job
caches `.cargo-target-cache` under a key that hashes only `Cargo.lock`, so a
runner image upgrade (MSVC 14.5x) does not invalidate the cached object, and
the linker rejects an object produced by the older compiler.

Fix: `cargo clean -p needle-sys` in a step before `cargo tauri build`. This is
the smallest blast radius; bumping the whole cache key would also work but
throws away every other crate's cache.

Status: reasoned from the linker error and the cache key; not yet exercised by
CI because the branch is unpushed.

## Guard added

`scripts/check-artifact-pins.sh` previously pinned only the llama.cpp commit
across three files. It now also requires:

- the release workflow's model URL to equal `PARAKEET_BASE_URL` + the
  `parakeet-tdt-v3` file name from `crates/teletype-speech/src/catalog.rs`;
- `TELETYPE_SELF_TEST_MODEL=parakeet-tdt-v3` in the workflow.

Verified both directions: the real repo prints two OK lines and exits 0; a
doctored copy of the workflow with the old `ggerganov/whisper.cpp` URL prints
`FAIL: release smoke model URL drift` and exits 1.

## Reusable lesson

1. A value copied outside its source of truth (the catalog URL inside a YAML
   file) needs a mechanical check, not care. Add it to the existing pins
   script at the same time as the fix.
2. A cargo build cache keyed only on `Cargo.lock` is implicitly keyed on the
   compiler and SDK too. When a linker error names a file restored from that
   cache after a runner upgrade, clean that one package rather than trusting
   the cache.
3. A cross-compile job needs every tool invocation that emits a binary to be
   told the target arch; `clang++` and `libtool` do not inherit it from cargo.
4. After fixing the first failure in a multi-job release, read the *other*
   failing jobs' logs before re-running; this run had three unrelated causes
   and the second was hidden behind the first.
