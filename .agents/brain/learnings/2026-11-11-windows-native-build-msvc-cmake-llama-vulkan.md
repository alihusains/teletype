---
id: L089
title: Native Windows build of teletype-desktop (MSVC + CMake + llama-server Vulkan prebuilt)
status: verified
type: build
area: windows-build
agent: pi
tool: cargo / cmake / llama.cpp b12200
date: 2026-11-11
---

# Native Windows build of teletype-desktop (MSVC + CMake + llama-server Vulkan prebuilt)

## Problem

`cargo build -p teletype-desktop` fails on a fresh Windows machine:
no MSVC C++ toolchain, no CMake, and the Tauri resource validator rejects
the build because `binaries/llama-server-win` does not exist.

## Symptoms

1. `cargo tauri` → `error: no such command: 'tauri'` (tauri-cli not installed).
2. `cargo install tauri-cli` → `link.exe returned an unexpected error` /
   `link: extra operand` — Git-for-Windows `link.exe` shadowed the MSVC one;
   no `cl.exe`/`link.exe` on PATH.
3. After toolchain install, `cargo build` → `fatal error C1056: cannot update
   the time date stamp field in '...<hash>-<file>.o'` in `lzma-sys` (and any
   crate that compiles C via cc-rs).
4. After that clears → `resource path 'binaries\llama-server-win' doesn't exist`
   from the Tauri build script.

## Initial hypotheses

1. Broken MSVC install / missing linker → disproven: toolchain verified present.
2. Antivirus locking `.o` files → disproven: Defender service disabled.
3. Filesystem filter driver (CrowdStrike `CSAgent` present) intermittently
   locking freshly-written `.o` under high parallelism → consistent with
   C1056 being file-specific and non-reproducible in isolation.

## Investigation

1. Confirmed VS Build Tools installed under
   `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools` (the
   x86 path, not `C:\Program Files`).
2. Verified `cl.exe`/`link.exe` (MSVC 14.44.35207) and `cmake.exe`
   (`Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin`).
3. Installed the llama-server Vulkan prebuilt exactly as `release.yml` does:
   download `llama-b12200-bin-win-vulkan-x64.zip` from the llama.cpp
   `b12200` release, unpack into `crates/teletype-desktop/binaries/llama-server-win/`,
   and copy `llama-server.exe` to `binaries/llama-server`.
4. Rebuilt with `CARGO_BUILD_JOBS=4` and a bounded retry loop in
   `build-windows.bat`.

## Evidence

- `cl.exe`/`link.exe` present; `cmake.exe` present (`CMAKE-OK`).
- `binaries/llama-server-win/llama-server.exe --version` →
  `version: 0.9.0-dev (build 12200, commit c19043f67)` — matches the pinned
  llama.cpp commit `c19043f67e52f1f1e00d797246e19643bb2f168e`.
- `cargo build -p teletype-desktop` → `Finished dev profile ... in 2m 33s`,
  exit 0. Produced `target/debug/teletype.exe` (~85 MB).
- Launched `target/debug/teletype.exe`: window title `Teletype`,
  `hotkey registered: Ctrl+Alt+Space`, `paste-last hotkey registered: Ctrl+Alt+V`,
  `speech model loaded (sherpa) — 3203 ms`.

## Root cause

Two independent blockers:
1. No native C/C++ toolchain → cc-rs cannot compile C deps (lzma-sys, ring, etc.).
2. Tauri bundles `binaries/llama-server-win/` as a resource directory; the
   directory must exist at build time or the resource validator fails. The
   Windows llama-server is a build artifact (not tracked), so a fresh checkout
   lacks it.

The C1056 stamp errors are a secondary, intermittent file-locking issue under
high build parallelism on this host (CrowdStrike `CSAgent` minifilter is
present); lowering `CARGO_BUILD_JOBS` plus a retry loop works around it.

## Fix

1. Install VS Build Tools with the C++ workload + CMake component:
   `Microsoft.VisualStudio.Workload.VCTools` +
   `Microsoft.VisualStudio.Component.VC.CMake.Project`.
2. Build via `build-windows.bat`, which calls `vcvars64.bat` first (puts the
   correct `cl.exe`/`link.exe` on PATH ahead of Git-for-Windows' `link.exe`),
   sets `CARGO_BUILD_JOBS=4`, and retries up to 5× on failure.
3. Provision the llama-server Vulkan prebuilt into
   `crates/teletype-desktop/binaries/llama-server-win/` (and copy
   `llama-server.exe` to `binaries/llama-server`), mirroring `release.yml`.

## Verification

- `cargo build -p teletype-desktop` exits 0.
- `target/debug/teletype.exe` launches, registers hotkeys, loads the sherpa
  speech model, and opens the `Teletype` window.

## Reusable lesson

On Windows, a native `teletype-desktop` build needs three things a fresh
checkout does not have: (a) an MSVC C++ toolchain + CMake on PATH via
`vcvars64.bat`, (b) the llama-server Vulkan prebuilt provisioned into
`crates/teletype-desktop/binaries/llama-server-win/`, and (c) a reduced
`CARGO_BUILD_JOBS` to dodge intermittent C1056 `.o` stamp locks. `build-windows.bat`
encodes all three. Windows speech runs the sherpa-onnx Parakeet engine
(`sherpa-parakeet-v3-int8`); the ggml/whisper.cpp Parakeet and whisper-rs
backends are macOS-only.

## Evidence / sources

- Commit: build run on `main` @ `5656fb0` (v0.2.18-15).
- Documentation: `.github/workflows/release.yml` (Windows llama-server Vulkan
  prebuilt step, `b12200`); `scripts/build-llama-server.sh` (macOS path +
  the `binaries/llama-server-win` resource note); `crates/teletype-desktop/tauri.conf.json`
  (resource map); `crates/teletype-inference/src/server.rs` (runtime exe path).
- User confirmation: app launched and running on the Windows host 2026-11-11.

## Promotion

Promote to `verified` only when the learning is supported by evidence or reviewed.
