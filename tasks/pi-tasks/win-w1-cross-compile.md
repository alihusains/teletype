# Task: Windows-W1 — Cross-platform compile gate (cargo check --target x86_64-pc-windows-msvc)

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo.
- Paste the REAL output of every verification command in your final report, not prose.

## Files you may edit
- `crates/teletype-desktop/Cargo.toml` and `crates/teletype-desktop/build.rs`
  (platform-conditional dependencies)
- `crates/teletype-desktop/src/lib.rs`, `main.rs`, `typing.rs`, `dictation.rs`,
  `tray.rs`, `overlay.rs`, `secrets.rs` (cfg gates ONLY — no behavior changes)
- `crates/teletype-core/**` and `crates/teletype-inference/**` and
  `crates/teletype-speech/**` (cfg gates ONLY where a crate genuinely cannot
  build for Windows)
- `.github/workflows/ci.yml` (add the Windows check job)

## Why
"Production-ready Windows" starts with the bar: the workspace must COMPILE for
Windows. Today the macOS `.m` sources are compiled unconditionally in
`build.rs` (they are already behind `#[cfg(target_os = "macos")]` at line 4 of
build.rs — verify), but nothing has ever been cross-compiled, so there are
almost certainly ungated macOS-only assumptions (link names, `objc2`/`core-graphics`
deps, cpal host assumptions, keyring backends) that will surface as compile
errors. This task makes the compile gate real and green. It does NOT require a
Windows machine; it is a cross-compile check.

## Read first
1. `crates/teletype-desktop/build.rs` (whole file): the `#[cfg(target_os = "macos")]`
   block that compiles the four `.m` files. Confirm the gate exists; if any `.m`
   compile is outside the gate, fix that first.
2. `Cargo.toml` (workspace) + each crate's `Cargo.toml`: list every dependency
   that is platform-sensitive: `objc2*`, `core-graphics*`, `cocoa*`,
   `core-foundation*`, `cpal`, `keyring`, `enigo`, `arboard`, `whisper-rs`,
   `parakeet-sys`, `tauri-apple-intelligence`.
3. `crates/teletype-speech/build.rs` (whole file): it already branches on
   `cargo_target_os`; read what it does for non-macos (line 98+) and what it
   downloads (whisper.cpp). Note anything that would fail on a Windows host.
4. `crates/teletype-speech/src/parakeet*` and `crates/parakeet-sys`: Parakeet is
   CoreML-based and macOS-only. It must become a macOS-only feature.

## Build
1. Add the Windows target (one-time, on this Mac):
   `rustup target add x86_64-pc-windows-msvc`
2. Run `cargo check --workspace --target x86_64-pc-windows-msvc` and fix every
   error, in this order of preference:
   a. Move genuinely macOS-only deps to
      `[target.'cfg(target_os = "macos")'.dependencies]` in the crate's
      Cargo.toml (e.g. `tauri-apple-intelligence`, any `objc2`/`core-graphics`
      direct dep, `parakeet-sys`).
   b. Gate the corresponding `mod` / `use` / impl with `#[cfg(target_os = "macos")]`
      and provide a Windows stub that returns a clear
      `"not available on Windows"` error where the trait requires it (follow the
      existing `GenericPlatform` pattern in `crates/teletype-desktop/src/platform/mod.rs`).
   c. For `parakeet-sys` / Parakeet provider: gate the provider behind
      `#[cfg(target_os = "macos")]` and make the provider registry on Windows
      list only Whisper (and the LLM providers, which are cross-platform).
   d. `cpal`: should compile on Windows as-is (Windows WASAPI host); if it does
      not, gate the audio capture module and report exactly what failed.
   e. Do NOT stub out the injector: `enigo` + `arboard` are cross-platform and
      the injector in `crates/teletype-core/src/injector.rs` already uses them —
      it must compile for Windows.
3. `tauri.conf.json`: confirm the bundle config does not hard-fail on Windows
   (tauri handles both; just confirm no macOS-only bundle keys break the build).
4. Add a CI job to `.github/workflows/ci.yml`:

       windows-compile:
         name: Windows cross-compile check
         runs-on: macos-latest
         steps:
           - uses: actions/checkout@v4
           - uses: dtolnay/rust-toolchain@stable
             with:
               toolchain: "1.88"
               targets: "x86_64-pc-windows-msvc"
           - name: cargo check (windows msvc)
             run: cargo check --workspace --target x86_64-pc-windows-msvc

   (msvc cross from macOS needs no extra linker for `cargo check`; if the
   whisper.cpp/parakeet build scripts still run and fail, make them no-op for
   the windows target in their build.rs and say so in the report.)

## Hard rules
- NO behavior changes on macOS. The macOS build and its tests must stay green:
  run `cargo test -p teletype-core --lib && cargo test -p teletype-desktop --lib`
  at the end and paste the output.
- Do not delete macOS functionality; gate it.
- Do not add Windows runtime code (SendInput, registry, etc.) — W2/W3 own that.

## Verification (paste real output)
1. `rustup target list --installed` (shows x86_64-pc-windows-msvc)
2. `cargo check --workspace --target x86_64-pc-windows-msvc 2>&1 | tail -5`
   (must end in `Finished`)
3. `cargo test -p teletype-core --lib 2>&1 | tail -3`
4. `cargo test -p teletype-desktop --lib 2>&1 | tail -3`
5. `git diff --stat` (so the reviewer can see the blast radius)
