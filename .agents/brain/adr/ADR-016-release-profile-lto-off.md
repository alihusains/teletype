# ADR-016 — The release profile runs with LTO off

**Status:** accepted
**Date:** 2026-10-23
**Decider:** Qoder (CI/CD mandate: the pipeline must stay green and the release must ship)
**Supersedes:** the shipping-profile half of L036's rule ("workspace keeps `lto = true`")

## Decision

`[profile.release] lto = false` in the workspace root `Cargo.toml`, with
`codegen-units = 1` retained. The macOS shipping binary cannot link with LTO on
in any mode, so the profile default is the value that ships.

## Context

The v0.2.9 release failed at `cargo tauri build`. The blocking `link-macos` job
added in `c8eaaa0` reproduced the failure class on a branch push, and the run
that followed it turned red on the LTO error itself:

```
error: failed to get bitcode from object file for LTO (Can't find section __bitcode)
```

`teletype-desktop` links static archives that rustc did not produce:
parakeet/ggml C++ (`parakeet-sys`, `parakeet-cpp-sys`) and the FluidAudio Nemo
`libtext_processing_rs.a` xcframework slice (`teletype-fluid`). Apple's LTO pass
requires every input object to carry matching LLVM bitcode; native-code archives
carry none. ThinLTO asks the linker the same question and failed with the same
message, which is the result that makes this a profile decision rather than a
tuning decision.

## Alternatives considered

- **`lto = "thin"`** (tried first, on the theory that fat LTO was the problem):
  failed identically. Rejected on evidence, not preference.
- **Per-crate override only, as L036 recommended** (`--config
  profile.release.lto=false` at the call site): rejected. The failure is on the
  app binary that `release.yml` builds, so an override would have to be threaded
  through `cargo tauri build`, which is the exact invocation that must not be
  special-cased. Global default is the honest place for a global constraint.
- **Keep LTO and restructure the foreign deps into dylibs**: rejected for this
  round. It might restore the inlining, but it changes how the app is packaged
  and signed (see D010 on unsigned builds, and the rpath work in L015) while the
  goal was a release that ships.
- **Split profiles** (e.g. a `release-lto` profile for pure-Rust binaries):
  rejected as unused complexity. Nothing ships a Rust-only binary today.

## Consequences

- The app binary is larger and Rust-side hot paths lose cross-crate inlining.
  The measured runtime of a dictation take is dominated by ASR and the polish
  model, both foreign code, so the loss is not on the critical path. This should
  be re-measured against `evals/` latency numbers if a Rust-side regression
  appears; the bench job already carries a criterion baseline.
- Any future proposal to enable LTO is a one-command test, not an argument:
  `cargo build --release -p teletype-desktop --bin teletype`. `link-macos`
  already runs it on every push to a release branch, so a wrong answer goes red
  in about ten minutes instead of at the tag in about thirty.
- Introducing another clang/Swift static archive into the app graph does not
  change this decision; introducing a *pure-Rust* shipping binary does not
  either, because the constraint is per-link, not per-workspace.
- L036's `teletype-eval` override is now redundant but harmless; the
  self-healing build in `scripts/eval/run_bakeoff.sh` is left alone so that the
  script works on older branches too.

## Evidence

Commit `0b9029d`. Local reproduction of all three configurations on the same
toolchain (fat: fail, thin: fail, off: exit 0 with `_nemo_tn_fst` and
`_nemo_free_string` defined per `nm`). CI run 39893783804 `link-macos` failure
before the change. See L049 and L050.
