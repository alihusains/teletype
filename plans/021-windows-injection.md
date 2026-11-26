# Plan 021: Windows hotkey and text-injection seam

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 8746c1c..HEAD -- crates/teletype-desktop/src/dictation.rs crates/teletype-core/src/delivery.rs docs/windows-from-mac.md`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: none
- **Category**: direction
- **Planned at**: commit `8746c1c`, 2026-11-26

## Why this matters

Teletype ships a Windows NSIS build (see `release.yml`), but the two
platform-critical capabilities — global hotkey capture and text injection —
are macOS-only. On Windows, the hotkey is handled by Tauri's global-shortcut
plugin (which works), but the text injection path (`delivery.rs`) uses
macOS Accessibility API (AXUIElement) and CGEvent, neither of which exists
on Windows. The `docs/windows-from-mac.md` file documents the gap.

This plan adds the Windows seam: a `TextInjector` trait with a macOS
implementation (existing code) and a Windows implementation (SendInput
API). The hotkey path is already cross-platform via Tauri; the injection
path is the blocker.

**This is a large plan (L effort, HIGH risk)**. It requires Windows-specific
code that cannot be tested on macOS. The plan is structured so that the
trait extraction and macOS refactor can be done and tested on macOS first;
the Windows implementation is a separate step that requires a Windows
machine.

## Current state

- `crates/teletype-core/src/delivery.rs` — the delivery/injection logic.
  Search for `#[cfg(target_os = "macos")]` and `#[cfg(target_os = "windows")]`
  to see what platform gates exist.
- `crates/teletype-desktop/src/dictation.rs` — the dictation pipeline that
  calls `delivery::deliver()`.
- `crates/teletype-desktop/src/hotkey_capture.m` — macOS-only Objective-C
  hotkey capture (CGEventTap).
- `crates/teletype-desktop/src/hotkey_latch.rs` — cross-platform hotkey
  state machine.
- `docs/windows-from-mac.md` — documents the Windows gaps.
- `.github/workflows/release.yml` — the Windows build job (NSIS bundle).

**Windows text injection options**:
1. **SendInput** (user32.dll) — sends keyboard events. Works everywhere,
   no accessibility permission needed. Slower for long text (sends one
   key event per character).
2. **SetClipboard + Ctrl+V** — copies text to clipboard, sends Ctrl+V.
   Faster for long text, but clobbers the user's clipboard (the macOS code
   already handles clipboard restore via change-count).
3. **UI Automation (UIA)** — sets the value of the focused control directly.
   Fastest, no clipboard clobber, but requires the target app to support
   UIA (most do, but not all).

**Recommended**: Option 2 (clipboard + Ctrl+V) for the initial seam, with
the existing clipboard-restore logic from macOS. It is the simplest, works
in all apps, and reuses the clipboard-restore code from plan 001.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Build (macOS) | `cargo build -p teletype-core -p teletype-desktop` | exit 0 |
| Core tests | `cargo test -p teletype-core --lib delivery` | all pass |
| Build (Windows) | `cargo build --target x86_64-pc-windows-msvc -p teletype-core` | exit 0 (requires Windows or cross-compile) |

## Scope

**In scope**:
- `crates/teletype-core/src/delivery.rs` — extract a `TextInjector` trait,
  add a Windows implementation
- `crates/teletype-core/Cargo.toml` — add `windows` crate dependency
  (`windows-sys` or `windows` crate) for SendInput/SetClipboard
- `crates/teletype-desktop/src/dictation.rs` — wire the Windows injector
- `docs/windows-from-mac.md` — update the status

**Out of scope**:
- The hotkey path (already cross-platform via Tauri global-shortcut)
- The audio capture path (already cross-platform via cpal)
- The ASR/ITN/polish pipeline (already cross-platform)
- The Windows build pipeline (already exists in `release.yml`)

## Git workflow

- Branch: `advisor/021-windows-injection`
- Commit message: `feat(delivery): add Windows text injection seam (clipboard + Ctrl+V)`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: Extract a TextInjector trait from delivery.rs

Read `crates/teletype-core/src/delivery.rs` in full. Identify the macOS
injection code (AXUIElement, CGEvent, clipboard). Extract it behind a trait:

```rust
pub trait TextInjector: Send {
    /// Inject text into the focused application.
    fn inject(&mut self, text: &str) -> Result<InjectOutcome, InjectError>;
    /// Restore the clipboard (called after inject, if we clobbered it).
    fn restore_clipboard(&mut self) -> Result<(), InjectError>;
}
```

The existing macOS code becomes `MacosInjector`. The `deliver()` function
takes a `&mut dyn TextInjector` instead of calling macOS-specific code
directly.

**Verify**: `cargo build -p teletype-core` → exit 0. `cargo test -p teletype-core --lib delivery` → all pass (macOS behavior unchanged).

### Step 2: Add the windows crate dependency

In `crates/teletype-core/Cargo.toml`, add:

```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.59", features = [
    "Win32_UI_WindowsAndMessaging",
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_System_Memory",
    "Win32_System_Ole",
] }
```

(`windows-sys` is lighter than the full `windows` crate and is the standard
choice for FFI to Win32 APIs.)

**Verify**: `cargo build -p teletype-core` → exit 0 on macOS (the dependency
is target-gated, so it is not compiled on macOS).

### Step 3: Implement WindowsInjector

In `crates/teletype-core/src/delivery.rs` (or a new
`crates/teletype-core/src/delivery_windows.rs`), add:

```rust
#[cfg(target_os = "windows")]
pub struct WindowsInjector {
    clipboard_restored: bool,
}

#[cfg(target_os = "windows")]
impl TextInjector for WindowsInjector {
    fn inject(&mut self, text: &str) -> Result<InjectOutcome, InjectError> {
        // 1. Save the current clipboard (for restore).
        // 2. Set the clipboard to `text`.
        // 3. Send Ctrl+V via SendInput.
        // 4. Wait a brief moment for the paste to complete.
        // 5. Mark clipboard_restored = true (restore is called separately).
        // ...
    }

    fn restore_clipboard(&mut self) -> Result<(), InjectError> {
        // Restore the saved clipboard content.
        // ...
    }
}
```

The clipboard save/restore logic can be shared with the macOS code
(extract it into a helper function that uses the platform clipboard API).

**Verify**: `cargo build --target x86_64-pc-windows-msvc -p teletype-core` → exit 0 (requires the Windows target to be installed: `rustup target add x86_64-pc-windows-msvc`). If you do not have a Windows machine or cross-compile setup, this step is **STOP** — report that the Windows build cannot be verified.

### Step 4: Wire the Windows injector in dictation.rs

In `crates/teletype-desktop/src/dictation.rs`, where the injector is created
(currently macOS-only), add a `cfg(windows)` branch that creates a
`WindowsInjector`.

**Verify**: `cargo build -p teletype-desktop` → exit 0 on macOS.

### Step 5: Update docs/windows-from-mac.md

Update the document to reflect that text injection now works on Windows
(clipboard + Ctrl+V). Note the limitations: (a) clipboard is briefly
clobbered (restored after), (b) apps that intercept Ctrl+V may not receive
the paste, (c) no AX-equivalent verification (the macOS code verifies the
paste via AX; Windows has no equivalent in this implementation).

**Verify**: `docs/windows-from-mac.md` reflects the new status.

## Test plan

- macOS: all existing delivery tests pass (the trait extraction is
  behavior-preserving).
- Windows: `cargo build --target x86_64-pc-windows-msvc -p teletype-core`
  compiles (compile-only test; runtime testing requires a Windows machine).
- `cargo test -p teletype-core --lib delivery` → all pass.

## Done criteria

- [ ] `TextInjector` trait is defined in `delivery.rs`
- [ ] `MacosInjector` implements the trait (existing behavior preserved)
- [ ] `WindowsInjector` implements the trait (clipboard + Ctrl+V)
- [ ] `windows-sys` dependency is target-gated to Windows
- [ ] `cargo build -p teletype-core` exits 0 on macOS
- [ ] `cargo build --target x86_64-pc-windows-msvc -p teletype-core` exits 0 (if Windows target available)
- [ ] `cargo test -p teletype-core --lib delivery` passes
- [ ] `docs/windows-from-mac.md` updated
- [ ] No files outside the in-scope list are modified
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- You do not have a Windows machine or cross-compile setup to verify the
  Windows build. The trait extraction and macOS refactor can be completed
  and committed; the Windows implementation is left as a stub with a
  `todo!()` and a comment explaining what is needed.
- The existing `delivery.rs` structure is too tightly coupled to macOS APIs
  to extract a trait without a large refactor (report the coupling and
  propose an alternative approach).
- The `windows-sys` crate version is incompatible with the Rust toolchain.
- A step's verification fails twice after a reasonable fix attempt.

## Maintenance notes

- The Windows injector uses clipboard + Ctrl+V. If a user reports that a
  specific app does not receive the paste, the fallback is SendInput
  (character-by-character keyboard events) — slower but more universal.
- The clipboard-restore logic from plan 001 (change-count guard) should be
  reused on Windows. The Windows clipboard does not have a change count
  equivalent to macOS; use a timestamp-based guard instead (if the clipboard
  was modified within 100ms of our write, skip the restore).
- This plan does NOT add Windows hotkey support — the Tauri global-shortcut
  plugin already handles that cross-platform. The macOS `hotkey_capture.m`
  (CGEventTap) is used for the more advanced hotkey features (latch,
  capture); if those are needed on Windows, a separate plan is required.
