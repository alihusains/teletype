# Task: Windows-W3 — Windows production hardening: tray, hotkeys, injection, packaging

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo.
- Paste the REAL output of every verification command in your final report, not prose.
- DEPENDS ON: Windows-W1 (cross-compile) green. W2 (typing hook) may run in
  parallel; you must NOT touch `typing.rs` or `typing_hook.rs`.

## Files you may edit
- `crates/teletype-desktop/src/tray.rs` (Windows tray behavior)
- `crates/teletype-desktop/src/dictation.rs` (hotkey fallback path ONLY)
- `crates/teletype-desktop/src/overlay.rs` (pill window on Windows)
- `crates/teletype-desktop/src/lib.rs` (Windows-only setup branches)
- `crates/teletype-desktop/tauri.conf.json` (bundle metadata for Windows)
- `crates/teletype-core/src/injector.rs` (Windows injection hardening ONLY)
- `scripts/` (new: Windows llama-server build script)
- `.github/workflows/ci.yml` (Windows build job)

## Why
Compile-green (W1) is not production-ready. This task closes the gaps that make
Windows a real target: reliable tray, hotkey fallback when the Fn-only hotkey is
unavailable, injection that works in elevated/odd apps, and a Windows build in
CI producing an installer.

## Read first
1. `crates/teletype-desktop/src/tray.rs` (whole file): how the tray is built on
   macOS (Tauri `tray-icon` is cross-platform; check what is platform-specific:
   icons, menu, activation policy).
2. `crates/teletype-desktop/src/dictation.rs` lines 108-150: `register_hotkey` —
   the Fn-only path is macOS-only and returns an error elsewhere. Windows needs
   a sane default and a clear error for Fn.
3. `crates/teletype-core/src/injector.rs` (whole file): the clipboard +
   paste-shortcut cascade via enigo/arboard. On Windows the paste shortcut is
   Control+V (already in `platform/windows_impl.rs`). Check the clipboard
   restore timing (the roadmap P2.1 lesson: restore must not block dictation
   completion).
4. `crates/teletype-desktop/tauri.conf.json`: current bundle config (identifier,
   icons, macOS-specific keys).
5. `scripts/build-llama-server.sh`: the macOS llama-server build (pinned commit
   `f95b0d95394d5e311ba8228689972843178c5e28`, static, OpenSSL/curl off).

## Build
1. **Tray**: verify `tray::build` compiles and behaves on Windows (it should,
   via tauri-plugin). Windows specifics: set activation policy so the app runs
   in the background (no taskbar button) when `show_tray_icon` is on — check
   how macOS does it (`set_activation_policy` or Tauri config) and add the
   Windows equivalent via the `windows` crate (`SetProcessDpiAwareness` is NOT
   this; use `ShowWindow`/activation via tao if Tauri does not expose it — check
   Tauri's `macOSPrivateApi`-equivalent for Windows, e.g. the `windows`
   attribute in tauri.conf.json). Document what you found.
2. **Hotkeys**: on Windows, bare `Fn` is not a global-shortcut key. In
   `register_hotkey`, for the non-macos path when `hotkey == "Fn"`, return a
   clear error message: "Fn alone is not supported on Windows; use
   Ctrl+Shift+Space or another combination". The default hotkey from
   `platform/windows_impl.rs` is already `Ctrl+Shift+Space` — verify the
   settings default resolves to it for new Windows users (check where the
   default is applied on first launch).
3. **Injection hardening (Windows)**:
   - After the Control+V, verify the text landed when possible (the macOS path
     has caret revalidation per roadmap P2.1; mirror what exists, do not build
     UIA automation — that is P2.1's own task).
   - Clipboard restore: ensure the restore runs after a bounded delay and does
     not await on the dictation completion path (read the current code; if it
     already matches, say so and change nothing).
   - Log the target window title (truncated, no sensitive content) at
     `tracing::debug!` to make the manual Windows matrix testable.
4. **Packaging**:
   - `tauri.conf.json`: add Windows bundle metadata (installer type NSIS is
     Tauri's default; confirm `bundle.windows` / `nsis` keys are sane, set
     `installMode` to per-user to avoid admin UAC on install).
   - Icons: confirm the `.ico` requirement (Tauri needs `icons/icon.ico`; check
     `crates/teletype-desktop/icons/` — if missing, generate from the existing
     PNG with `tauri icon` and commit the result).
   - `scripts/build-llama-server.ps1`: port of the macOS script for Windows
     (same pinned commit, `-DGGML_CUDA=OFF -DLLAMA_OPENSSL=OFF -DLLAMA_CURL=OFF`,
     static, output to `crates/teletype-desktop/binaries/`). It will be RUN on
     a Windows runner, so keep it simple and idempotent.
5. **CI**: add to `.github/workflows/ci.yml`:

       windows-build:
         name: Build Windows app
         runs-on: windows-latest
         if: github.event_name == 'push' && github.ref == 'refs/heads/main'
         steps:
           - uses: actions/checkout@v4
           - uses: dtolnay/rust-toolchain@stable
             with: { toolchain: "1.88" }
           - uses: actions/setup-node@v4
             with: { node-version: 20, cache: npm, cache-dependency-path: ui/package-lock.json }
           - name: Build llama-server
             shell: pwsh
             run: ./scripts/build-llama-server.ps1
           - name: Build Tauri app
             run: cargo tauri build
           - uses: actions/upload-artifact@v4
             with:
               name: teletype-windows
               path: crates/teletype-desktop/target/release/bundle/nsis/*.exe

   Note: `cargo tauri build` on windows-latest needs the tauri-cli installed the
   same way the macOS job does (copy the working pattern from the existing
   release-macos job, including the `cargo install tauri-cli` step).

## Hard rules
- Do not touch `typing.rs` / `typing_hook.rs` (W2 owns them).
- Do not implement UIA/AX-style deep injection (that is P2.1).
- macOS build and tests must stay green: run
  `cargo test -p teletype-core --lib && cargo test -p teletype-desktop --lib`
  and paste the tail.
- No secrets, no signing keys in the repo (signing is a separate ops task;
  note in the report that the NSIS installer will be unsigned until then).

## Verification (paste real output)
1. `cargo check --workspace --target x86_64-pc-windows-msvc 2>&1 | tail -5`
2. `cargo test -p teletype-core --lib 2>&1 | tail -3`
3. `cargo test -p teletype-desktop --lib 2>&1 | tail -3`
4. `cargo build -p teletype-desktop 2>&1 | tail -3`
5. `ls crates/teletype-desktop/icons/` (shows icon.ico present)
6. `pwsh -NoProfile -Command "Get-Content scripts/build-llama-server.ps1 | Select-Object -First 5"`
   (syntax sanity; the real run happens in CI on windows-latest)
7. In the report, list the manual Windows test matrix for the next human with a
   Windows box: global hotkey, mic capture, injection into Notepad/VS Code/
   browser, tray show/hide, pill position, AutoText expansion.
