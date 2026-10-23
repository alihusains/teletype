# L054 — The Windows Needle prebuilt is a MinGW/libc++ archive, not an MSVC library

- **ID:** L054
- **Date:** 2026-10-23
- **Agent:** OpenCode (release triage)
- **Area:** `.github/workflows/release.yml` Windows job / `crates/needle-sys`
- **Status:** verified (v0.2.12 released green 2026-10-23, release run `39924951925`)

## Problem

`v0.2.7`, `v0.2.10` and `v0.2.11` built macOS and Intel green, then the
Windows job died at the final Tauri link:

```
libneedle_sys-<hash>.rlib(needle.cpp.obj) :
fatal error LNK1143: invalid or corrupt file: no symbol for COMDAT section 0x5
```

The publish job was skipped every time. The latest published release is still
`v0.2.6`.

## What it was not

The first diagnosis blamed a stale cargo cache: the cache key hashes only
`Cargo.lock`, so an MSVC image upgrade could leave an old `needle.cpp.obj`
behind. That was wrong.

- `v0.2.11` run `39912944568` logged `Cache not found for input keys:
  cargo-target-windows-...` and `Cache sccache ... not found`. A completely
  cold build still failed with the same `LNK1143`.
- The "Drop cached needle-sys objects" step ran on that cold cache and printed
  `Removed 0 cached needle-sys path(s)`.
- `cargo clean -p needle-sys` on `v0.2.10` printed `Removed 0 files` for the
  same reason: there was nothing to clean.

A `cargo clean` that removes zero files is a real general caution, but it is
not this bug and it is not evidence of a poisoned cache.

## Evidence

Downloaded the exact upstream file the build script fetches:
`https://huggingface.co/Cactus-Compute/needle3/resolve/main/windows-x86_64/libneedle.a`
(SHA-256 `22d95a38f34c225a6af115c40069ea61360766479222862f1fea8b615ea7718a`,
2,347,552 bytes).

- The archive has one object, `needle.cpp.obj` (2,137,672 bytes), COFF machine
  `0x8664`, 4,775 sections, 12,477 symbols.
- It is an **Itanium C++ ABI / libc++** object: `_ZNSt3__1...`, `std::__1`,
  `_ZdlPv`, `_Znwy`, `__cxa_*`, `__gxx_personality_seh0`, `.llvm_addrsig`.
- Its `.drectve` is 120,550 bytes with 1,150 `-exclude-symbols:` entries.
  `-exclude-symbols` is an lld-link option, not an MSVC `link.exe` option.
- 129 undefined externals are libc++/libc++abi members (`_ZdlPv`, `_Znwy`,
  `_ZNSt3__1...`, `__cxa_throw`, `_Unwind_Resume`), not MSVC CRT symbols.
- Upstream's `windows-x86_64/needle.exe` is MinGW-w64 + libc++abi: it contains
  the string `Mingw-w64 runtime failure` and imports only `KERNEL32.dll`,
  `WS2_32.dll`, and the UCRT `api-ms-win-crt-*` DLLs.
- `lld-link` (Rust's `rust-lld -flavor link`) accepts the object and links it
  into a DLL with `/force:unresolved`; MSVC 14.51 `link.exe` rejects the same
  bytes with `LNK1143`. But lld-link still leaves the 129 libc++ symbols
  unresolved unless the MinGW libc++/libc++abi/libunwind runtime is added.

## Root cause

The Windows artifact is built for **LLVM-MinGW + libc++**, not MSVC. The MSVC
linker cannot parse its COMDATs, and the MSVC C++ runtime cannot satisfy its
Itanium-mangled libc++ symbols. No cache cleanup or linker switch fixes that;
the object needs a MinGW libc++ runtime.

## Fix (v0.2.12)

Do not link the Windows archive into the MSVC app. `needle-sys` now:

- early-returns from `build.rs` on Windows, emits `cfg(needle_stub)`, and never
  downloads or links `libneedle.a`;
- compiles a pure-Rust stub with the same raw C API signatures, returning
  `-1`/`0` or a NUL-terminated "Needle is unavailable..." message.

`teletype-inference` and `teletype-speech` compile unchanged; at runtime
Needle/Whistle report unavailable and their callers fall back. macOS and Linux
still download and link the real archive.

## Verification

- `cargo check -p needle-sys --target x86_64-pc-windows-msvc`: passes; the
  build script prints the stub warning.
- `cargo check -p needle-sys` (macOS): passes; real path.
- `cargo check -p teletype-inference -p teletype-speech` (macOS): passes.
- Windows cross-check of the full inference/speech stack on macOS stops in
  `ring` (no Windows SDK headers); the Windows CI runner is the real check.
- The `v0.2.12` release run (cut 2026-10-23, run `39924951925`) was fully
  green: all three platform builds + `Publish release & updater manifest`
  passed. The Windows job log shows `Compiling needle-sys v0.2.12`, the stub
  warning (`needle-sys: Windows uses the pure-Rust stub; the upstream
  libneedle.a is a MinGW/libc++ archive`), no libneedle link, and the NSIS
  build + installer smoke pass. Published assets:
  `Teletype_0.2.12_x64-setup.exe` (24.7 MB), `Teletype.app.zip` +
  `Teletype_0.2.12_aarch64.dmg`, `Teletype-x86_64.app.zip` +
  `Teletype-x86_64.dmg`, and `latest.json` (unsigned, empty `platforms` —
  no `TAURI_SIGNING_PRIVATE_KEY` secret, so auto-update stays disabled).

## Rule

- A native prebuilt is only portable to the ABI/toolchain it was built for.
  Check the object's mangling, `.drectve`, and undefined symbols before blaming
  a cache or a linker flag.
- A cold cache that reproduces the failure disproves a cache theory. Check the
  cache restore line before adding cleanup steps.
- Prefer a contained stub over mixing MinGW libc++ into an MSVC binary when the
  feature is not usable on that platform yet.
