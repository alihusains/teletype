# Windows work from a Mac

This is the practical answer to "I only have a Mac, how do I finish Windows?".
It records what is already verified, what can only be checked on a real Windows
desktop, and the two free paths to get that desktop from macOS.

## 1. What is already true (verified in CI, not assumed)

The Windows layer is further along than "stubbed":

| Piece | Where | Status |
|---|---|---|
| Foreground-app detection | `crates/teletype-desktop/src/platform/windows_impl.rs` (`GetForegroundWindow` → `QueryFullProcessImageNameW`) | implemented |
| Microphone capture | `cpal` default device, same path as macOS | implemented |
| Text injection | `enigo` + `arboard` via `teletype-core` (tiered paste, `Ctrl+V`) | implemented |
| Typed AutoText key hook | `crates/teletype-desktop/src/typing/typing_windows.rs` (`WH_KEYBOARD_LL`, own message-pump thread) | implemented |
| Hotkey | Tauri global-shortcut plugin (`Ctrl+Shift+Space`) | implemented |
| Tray | Tauri tray | implemented |
| Build + pipeline smoke | `release.yml` `release-windows` runs the shipped `.exe --self-test` | green in v0.2.21 |
| Compile gate on every PR | `ci.yml` `windows-compile` | green |

## Current state (2026-11-27)

The last remaining "stub" on the delivery path is gone: the Windows
`FieldReader` is now a real **UIA reader** (`uia.rs` +
`delivery_io::WindowsFieldReader`). It provides:

- **selection readback** — `focused_control_text` (TextPattern →
  ValuePattern → Name) verifies pastes on writable Edit fields;
- **secure-field detection** — UIA `IsPassword`; dictation is refused
  before any side effect, and the field is never read;
- **conservative writable-focus** — UIA **Edit** control only, never a
  password field, and never a Document/Text control (browser content is a
  Document; it goes to the optimistic-paste path, matching macOS);
- **URL-bar guard** — best-effort allowlist on the control's AutomationId /
  ClassName / Name (`omnibox`, `urlbar`, `addresseditbox`, "address bar",
  "url bar", "address and search bar"); bare `address`/`url` do not match.

Injection, paste (Ctrl+V), and the clipboard adapter already shipped
(see the table above). The delivery cascade also now initializes COM per
worker thread (`CoInitializeEx`, MTA) before its first UIA query.

What is **still unverified**: nothing here has been exercised on a real
Windows desktop — a Mac and a `windows-latest` CI runner can only prove
the code compiles (the §3 checklist is the way to close that gap), and the
Windows test suite is still compile-only / blocked by needle-sys LNK1143.

So the gap is **not a missing implementation, it is missing evidence on a real
Windows desktop**. Nothing in this repo has ever run the app with a human
holding a hotkey, in a real window station, and watched text land.

README.md's Status section now reflects this table: the remaining gap is
hardware evidence from a real Windows desktop, not missing implementation.

## 2. What a Mac cannot check, and what it can

A Mac can build nothing Windows-native: the C speech engines (whisper.cpp,
ggml Parakeet) and the Win32 APIs need the MSVC toolchain, so cross-compiling
from macOS is not a path worth taking. Neither can a `windows-latest` CI runner
show a GUI: it runs in Session 0 with no interactive desktop, so a Tauri window
can be created but never focused, typed into, or clicked. Treat a green Windows
CI run as "it compiles and the pipeline runs", never as "dictation works".

That leaves two free ways to get an interactive Windows desktop from a Mac.

### Path A — Windows 11 ARM in UTM (free, local, recommended first)

1. Install [UTM](https://mac.getutm.app/) (free, open source; a Mac App Store
   version exists too).
2. Create a VM: **Windows 11 ARM64 ISO** from Microsoft's Windows Insider/ARM
   evaluation downloads, 4 CPU / 8 GB RAM / 64 GB disk. UTM's Windows 11
   template handles the TPM and Secure Boot requirements.
3. Inside the VM install, all free:
   - Rust via `winget install Rustlang.Rustup` (MSVC toolchain)
   - **Visual Studio Build Tools** with "Desktop development with C++"
     (needed for `link.exe`; Rust's MSVC target cannot link without it)
   - Node 24 and `cargo install tauri-cli --version "2.12.0" --locked`
   - Git
4. Build and run in the VM:
   ```powershell
   cargo tauri build --bundles nsis
   .\target\release\teletype.exe
   ```
   For a fast inner loop use `cargo tauri dev` instead of a full bundle.

An ARM VM runs the app on ARM64 Windows, which is the same architecture
question the app already answers on Apple Silicon (ggml has an ARM64 build, and
Whisper is the Windows ASR engine; Parakeet/CoreML is macOS-only). Expect
slower inference than native x86_64 hardware — measure nothing about
performance in the VM, only behaviour.

### Path B — a real PC or a cloud Windows desktop

Only needed for performance numbers, GPU behaviour, or gaming-grade audio
drivers. A used mini-PC with 16 GB RAM is cheaper than a month of cloud
desktop for the hours this takes. This is the only path that produces
trustworthy latency numbers, so do the behaviour work in the VM and the
performance work here.

## 3. The checklist that actually closes the gap

Run this once in the VM against a fresh profile. Each line is a PASS/FAIL you
can write into a doc; the first one that fails is the next bug.

1. **Install**: the NSIS installer runs, the app launches, the tray icon appears.
2. **Capture**: the first dictation prompts for microphone access and
   `ms-settings:privacy-microphone` opens from the app's setup notes.
3. **Hotkey**: `Ctrl+Shift+Space` starts and stops a take; the pill shows.
4. **Transcription**: speech becomes text in the History screen.
5. **Injection**: the text lands in Notepad, in a browser address bar, in
   VS Code, and in an elevated app (expected to fail without matching
   elevation — record whether the app explains that).
6. **Non-Latin / IME**: a take with an IME active must not type the romanized
   form.
7. **Typed AutoText**: typing a trigger in Notepad expands it (this exercises
   `typing_windows.rs`, which CI cannot reach).
8. **Ducking**: volume lowers during a take and restores after, and after a
   kill -9 mid-take (the recovery spool).
9. **Long run**: 100 hotkey cycles and a sleep/resume cycle; the app stays
   responsive (this is the Windows equivalent of the main-thread gate).
10. **Model download**: pick a model, download, verify the checksum, and
    confirm the app survives a cancelled download.

## 4. What to fix in CI while you are here

Two cheap additions, once the VM checklist has found the first real bug:

- Run `teletype --self-test` on `windows-latest` for **every PR** (today only
  the release job does it), so a Windows pipeline regression cannot reach a tag.
- Add a `windows-latest` job that runs the `--e2e` file-transcription path on a
  committed WAV fixture. It needs no desktop and proves decode → ASR → cleanup
  on Windows end to end.

Neither replaces the VM: they only stop the "it compiles" claim from rotting.
