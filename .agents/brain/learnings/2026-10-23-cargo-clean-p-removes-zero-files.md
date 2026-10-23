# L054 — `cargo clean -p` can remove zero files and leave the stale object

- **ID:** L054
- **Date:** 2026-10-23
- **Agent:** OpenCode (release triage)
- **Area:** `.github/workflows/release.yml` Windows job / `needle-sys` cargo cache
- **Status:** candidate (fix in v0.2.11; verify the run)

## Problem

The `v0.2.10` release (`39908390917`) had macOS and Intel green, then the
Windows job died with:

```
libneedle_sys-a663f919e1529c33.rlib(needle.cpp.obj) :
fatal error LNK1143: invalid or corrupt file: no symbol for COMDAT section 0x5
```

The workflow already had a step meant to prevent exactly this:

```powershell
cargo clean -p needle-sys
```

Its entire output was:

```
     Removed 0 files
```

and the stale object was still linked. The publish job was skipped, so the
release never appeared. `v0.2.7` failed the same way.

## Root cause

`cargo clean -p <spec>` resolves the package through the **current** unit graph
and removes the artifacts that graph would produce now. The cached artifacts
carry an older metadata hash (the runner's MSVC was upgraded), so the package
matched but no current path did. The stale

- `release/deps/libneedle_sys-<oldhash>.rlib`
- `release/build/needle-sys-<oldhash>/out/.../needle.cpp.obj`
- `release/.fingerprint/needle-sys-<oldhash>/`

survived, and the linker used them. `cargo clean` exits 0 either way, so the
step looked successful.

A second trap: GitHub Actions caches are immutable per key. When the key hits,
the post-job save is skipped, so deleting files in the workspace does not
repair the cache — every run restores the same poisoned entry. The delete has
to run before every build, or the key has to change.

## Fix

Delete the three paths by glob before the build, using the job's own
`CARGO_TARGET_DIR`:

```powershell
foreach ($glob in @(
  "$cache\release\build\needle-sys-*",
  "$cache\release\deps\*needle_sys*",
  "$cache\release\.fingerprint\needle-sys-*"
)) { ... Remove-Item -Recurse -Force ... }
```

Then Cargo has no rlib and no fingerprint, so the build script re-runs and
compiles `needle.cpp` with the current toolchain. The longer-term fix is to put
the MSVC toolset version in the cache key, so an image upgrade cannot restore
objects built by a different compiler.

## Verification

- `release.yml` parses under PyYAML and passes `actionlint`.
- PowerShell is not installed on the macOS host, so the step itself cannot be
  executed locally; the v0.2.11 run (`39912944568`) must show
  `Removed N cached needle-sys path(s)` with `N > 0`, then a green Windows link.
- Do not mark this verified until that run publishes.

## Rule

- Read the `Removed N files` line. A successful `cargo clean -p` that removes
  zero files is evidence the artifact you fear is still there.
- For a poisoned build cache, delete by **path** (rlib + build `OUT_DIR` +
  fingerprint). `cargo clean -p` is for the package's current units, not for
  every hash that ever existed.
- A cache key that hashes only the lockfile is implicitly wrong for any native
  code: the compiler is an input too.
