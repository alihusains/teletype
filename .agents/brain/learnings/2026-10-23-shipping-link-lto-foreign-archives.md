# L049 — The shipping release link cannot use LTO at all; `thin` fails exactly like `true`

- **ID:** L049
- **Date:** 2026-10-23
- **Agent:** Qoder (CI/CD triage on the v0.2.9 failure class)
- **Area:** workspace `Cargo.toml` `[profile.release]`, `crates/teletype-desktop` final link
- **Status:** verified (local reproduction of both modes, and of the passing mode)
- **Supersedes:** the "keep `lto = true` for the shipping profile" rule in L036

## Problem

The v0.2.9 release died at `cargo tauri build`. The new blocking `link-macos`
CI job reproduced it on the branch before any tag:

```
error: failed to get bitcode from object file for LTO (Can't find section __bitcode)
```

while building `cargo build --release -p teletype-desktop --bin teletype`.

L036 had already documented this error for `teletype-eval` and concluded the
workspace profile should stay `lto = true`, with only the dev tool overridden.
That conclusion was wrong about scope: the *shipping* app binary hits the same
failure, so per-crate overrides cannot save it.

## Investigation

1. `lto = true` (fat) on `teletype-desktop`: failed as above.
2. `lto = "thin"` in the manifest: failed with the identical message. This is
   the important negative result. ThinLTO still asks the linker to read LLVM
   bitcode from every input object, so a foreign archive is just as fatal as
   in fat mode. Nothing in the error text distinguishes the two modes, so a
   "did you try thin?" guess costs a full release build (~6 min) to answer.
3. `lto = false`: exit 0, and `_nemo_tn_fst` / `_nemo_free_string` resolved
   (checked with `nm`: defined `T`, no undefined nemo symbols left).

## Root cause

`teletype-desktop`'s link graph pulls in static archives built by clang/Swift,
not rustc: parakeet/ggml C++ objects (`parakeet-sys`, `parakeet-cpp-sys`) and
the FluidAudio Nemo `libtext_processing_rs.a` xcframework slice
(`teletype-fluid`). Apple's linker only runs its LTO pass if every object
contributes a matching `__bitcode` section. Native-code archives have none, so
the pass aborts. This is a property of the *link inputs*, so it will reappear
whenever a new foreign static archive enters the app graph, regardless of which
LTO mode is set.

## Rule

- `[profile.release] lto = false` in the workspace root is the shipping
  configuration, not a temporary workaround. See ADR-016.
- `codegen-units = 1` stays; it is orthogonal to LTO and does not require
  bitcode from foreign objects.
- If someone proposes re-enabling LTO, the test is one command, not a debate:
  `cargo build --release -p teletype-desktop --bin teletype`. The `link-macos`
  job runs exactly that on every push to a release branch.
- The `--config profile.release.lto=false` CLI override from L036 is still
  valid and now redundant (the profile default already is `false`).
  `scripts/eval/run_bakeoff.sh` keeps its self-healing flag; harmless.
- Do not conclude "no errors" from a filtered build log. The failure prints a
  bare `error:`, never `error[E0xxx]`, so a `grep "error\["` or a
  `... | tail` pipeline reports success on a dead build. Redirect full output
  and check `EXIT=$?` (this bit me again on 2026-10-23, see L050).

## Verification

2026-10-23, same machine, same toolchain: `lto = true` and `lto = "thin"` both
failed with the bitcode error; `lto = false` produced the arm64 `teletype`
binary with the nemo symbols defined. The `link-macos` job turned red on run
39893783804 with this error before the profile change, which is the gate
working as designed.
