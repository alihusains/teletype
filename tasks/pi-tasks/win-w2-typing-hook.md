# Task: Windows-W2 — Windows runtime: auto-text watcher + typed-tap equivalent

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo.
- Paste the REAL output of every verification command in your final report, not prose.
- DEPENDS ON: Windows-W1 (cross-compile gate) must be green first. If
  `cargo check --workspace --target x86_64-pc-windows-msvc` fails when you start,
  STOP and report the errors; do not fix W1's scope here.

## Files you may edit
- `crates/teletype-desktop/src/typing.rs` (the Windows watcher path)
- `crates/teletype-desktop/src/typing_hook.rs` (NEW, Windows-only module)
- `crates/teletype-desktop/Cargo.toml` (Windows-only deps: `windows` crate)
- `crates/teletype-desktop/build.rs` (no `.m` on Windows; nothing to add unless
  a C helper is needed)
- `crates/teletype-desktop/src/lib.rs` (module declaration, cfg-gated)

## Why
The typed AutoText watcher (`typing.rs`) is macOS-only: it installs a passive
CGEventTap (`typing_tap.m`) and maps macOS virtual key codes to characters. The
watcher thread, buffer logic, trigger matching, and enigo expansion are all
cross-platform already (enigo works on Windows). What is missing is the
KEYSTROKE SOURCE on Windows. This task provides it with a low-level
`WH_KEYBOARD_LL` hook, mirroring the macOS tap's contract: a background source
feeds characters into the same channel the watcher already drains.

## Read first
1. `crates/teletype-desktop/src/typing.rs` (whole file, ~300 lines): the exact
   contract — `TAP_TX: OnceLock<Sender<i32>>`, `on_key_down` C callback,
   `KC_SPACE`/`KC_ENTER` constants, the watcher loop that drains `rx` every 50 ms,
   `lookup_replacement`, `expand`. Your Windows source must feed the SAME
   `rx` with the SAME semantics (character events, space/enter as delimiters).
2. `crates/teletype-desktop/src/fn_tap.rs` and `fn_tap.m`: the existing pattern
   for a platform-specific native helper behind a Rust facade (the Windows side
   will be pure Rust via the `windows` crate, no C file needed).
3. `crates/teletype-desktop/Cargo.toml`: check whether the `windows` crate is
   already a dependency (`grep windows Cargo.toml`); `platform/windows_impl.rs`
   already uses it for foreground-app detection, so the crate + needed features
   are likely present. Add features `Win32_UI_WindowsAndMessaging`,
   `Win32_System_Threading`, `Win32_System_LibraryLoader` as needed.

## Build
1. Refactor `typing.rs` minimally so the key source is pluggable:
   - Introduce `fn install_key_source(tx: Sender<i32>) -> Result<(), String>`
     with two impls: `#[cfg(target_os = "macos")]` (the existing
     `teletype_typing_tap_start` call, unchanged behavior) and
     `#[cfg(target_os = "windows")]` (the new hook). The `i32` on the channel
     becomes a small tagged enum if needed (e.g. keep `i32` = char code point
     for letters, and keep the existing `KC_SPACE`/`KC_ENTER` sentinel values so
     the watcher loop does not change). Document the encoding in the file header.
2. `typing_hook.rs` (Windows):
   - `SetWindowsHookExW(WH_KEYBOARD_LL, proc, GetModuleHandleW(null), 0)` on a
     DEDICATED thread with its own message loop (`GetMessageW` loop) — a LL
     hook only fires while the installing thread pumps messages.
   - In the hook proc: `KBDLLHOOKSTRUCT::vkCode` → map via `MapVirtualKeyW` +
     the keyboard layout (`ToUnicodeW`) to the unshifted character, mirroring
     `key_code_to_char`'s role. Send space (VK_SPACE) and enter (VK_RETURN) as
     the delimiter sentinels. Ignore events where `flags & LLKHF_INJECTED` is
     set (do not react to our own enigo expansion — the macOS side has the same
     self-event concern; check how it avoids it and match).
   - Hook proc must return in < 30 ms or Windows removes it: only a channel
     `try_send` + `CallNextHookEx` inside the proc. All mapping work that could
     block happens on the pump thread, not in the proc (queue the vkCode, map
     after).
   - `remove_hook()` on stop: `UnhookWindowsHookEx` + post a quit message to the
     pump thread; join it.
3. Wire into `typing.rs` start/stop exactly where the macOS tap is started/stopped
   (search for `teletype_typing_tap_start` / `_stop`).
4. The watcher loop, buffer, trigger matching, and `expand()` stay untouched.

## Hard rules
- Do not change macOS behavior: the macOS path must be byte-for-byte the same
  calls as before (refactor may move code, not change it).
- No new global hotkey or permission machinery (Windows has no Accessibility
  gate for LL hooks; note in the module doc that Admin/Elevated-target apps may
  not receive hook events — known Windows limitation, document it).
- Keep the file under 300 lines; if the hook grows, split pump/hook into two
  files.

## Verification (paste real output)
1. `cargo check --workspace --target x86_64-pc-windows-msvc 2>&1 | tail -5`
   (must end in `Finished`)
2. `cargo test -p teletype-desktop --lib 2>&1 | tail -3` (macOS tests still green)
3. `cargo build -p teletype-desktop 2>&1 | tail -3` (macOS build still green)
4. Unit tests (platform-independent parts only, since there is no Windows box
   here): the vkCode → character mapping function, the delimiter sentinels, and
   the buffer/trigger logic (already tested — confirm still green). State
   explicitly in the report: "runtime hook behavior NOT verified on Windows;
   needs a Windows machine" — do not claim otherwise.
