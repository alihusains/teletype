# Teletype Windows Roadmap

This is the plan to take Teletype from "compiles on Windows" to a production-grade
Windows dictation app. It is informed by the reference project's Windows port,
which carried a real Windows port through 23
phases with measured evidence. That reference used C#/WinUI 3; Teletype is Rust/Tauri,
so the *mechanisms* and *measured learnings* carry over, but the *implementation
language* does not.

The reference is the source of truth for "how hard is each piece on Windows" and for
the numbers that predict what will and will not work. Never treat a reference claim as
Teletype evidence; it is a prior, not a measurement.

---

## 1. Where Windows stands today (2026-09-26)

Committed on `main`:

- **W1 (commit `3c9783a`):** the Windows layer is no longer a stub for the typed-AutoText
  path. A `WH_KEYBOARD_LL` low-level keyboard hook
  (`crates/teletype-desktop/src/typing_windows.rs`) reports key-downs on its own
  message-pumping thread and feeds the same channel the macOS CGEventTap uses.
  `typing.rs` gained a platform-unified `decode_key()` (space/enter sentinels shared,
  `char` codepoint on Windows vs virtual-key-code on macOS).
- **Cross-platform speech:** `teletype-speech` is no longer macOS-gated. The
  engines split by what the dependency graph actually builds for Windows:
  `whisper-rs` (whisper.cpp), `parakeet-sys` (ggml Parakeet), and
  `parakeet-cpp-sys` are macOS-only dependencies (`crates/teletype-speech/Cargo.toml`),
  and the platform's Whisper provider is a stub that fails on load
  (`crates/teletype-speech/src/whisper/stub.rs`), so Windows does **not** run
  Whisper. `sherpa-onnx` is unconditional, so Windows resolves to the
  sherpa-onnx NeMo Parakeet int8 builds (`catalog.rs::pick_engine` →
  `sherpa-parakeet-v3-int8`). `dictation.rs::resolve_speech_model` maps the
  catalog engine to `SpeechEngineKind::Sherpa` and the extracted model
  directory.
- **CI gate:** `.github/workflows/ci.yml` runs a native `windows-latest` job
  (`cargo check --workspace` under MSVC). Cross-compiling from macOS is NOT viable
  (whisper.cpp/ring C code needs a native MSVC C toolchain). This is a compile gate,
  not a runtime gate.

Not yet done: a real Windows build/test/run, WASAPI capture, global push-to-talk
hotkey, clipboard-paste injection with restore, tray, packaging/updater, and any
native UAT on a Windows machine.

---

## 2. The two promises (what the roadmap is judged on)

Copied from the reference's `load-bearing-constraints.md`. Everything else is a limb.

1. **Sub-second transcription.** The reference measured the Mac fleet median at
   0.61s no-polish / 1.65s on-device polish. On Windows the reference measured
   (Spike S1, i9-14900KF + RTX 4090): tuned-CPU Parakeet int8 = 0.32-0.35s for a
   10s clip (beats the promise), 1.2-1.45s for 20s; CUDA fp32 (QDQ-free) = 0.119s
   for 10s, 0.654s for 95s. The sub-second bar is achievable on Windows but it is
   a *measurement*, not an assumption, and it depends on thread pinning (see
   learnings).
2. **Text lands in the app that has focus.** Clipboard + `SendInput` is the Windows
   workhorse paste route (the reference's Tier-2). UI Automation TextPattern is the
   higher tier but coverage is per-app and was UNMEASURED in the reference (their
   Spike S2). This is the biggest platform risk.

---

## 3. Phases

### Phase W0: Windows runtime proof (the gate for everything)

Goal: prove a Windows build actually runs the heart path end-to-end on a real Windows
machine before investing in limbs.

- Build the release app natively on Windows (or `windows-latest` CI) and launch it in a
  logged-in interactive desktop session (a service/RDP session will not draw).
- Confirm WASAPI capture, the global hotkey, sherpa-onnx Parakeet ASR, and
  clipboard-paste each work with a physical mic and a real target app.
- Record the numbers (capture rate, ASR latency, paste success) as the Teletype
  Windows baseline.

Exit: a screen recording of a real dictation landing in a real app on Windows.

### Phase W1: capture + hotkey + injection (the heart)

- WASAPI microphone capture at 16 kHz mono via the platform layer (replaces the
  macOS `cpal`/CoreAudio path). Re-derive the zero-signal / device-loss / route-change
  handling; do not port CoreAudio constants.
- Global push-to-talk hotkey (hold/release, cancel, debounce, no-overlap).
- `TextInjector` Windows impl: snapshot clipboard -> paste via `SendInput`/clipboard ->
  restore. Preserve all *formats of the current item* (the Win32 clipboard holds one
  item, not the multi-item macOS pasteboard; see learnings).
- Wire the existing `resolve_speech_model` sherpa path to a real model on
  Windows: download `sherpa-parakeet-v3-int8` and load the extracted directory
  (`<models_dir>/sherpa/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8`).

Exit: hold key, speak, release, clean text lands at the cursor in a normal app.

### Phase W2: typed AutoText + settings parity (mostly done)

- Typed AutoText watcher: DONE (W1 commit `3c9783a`). Verify it on Windows with the
  WH_KEYBOARD_LL hook against a real app.
- Settings parity: engine picker, language, VAD, pill style, transforms, styles,
  AutoText, dictionary, insights. Most are platform-free (teletype-core) and should
  just work; verify each in the Windows UI.

Exit: every macOS settings screen works identically on Windows.

### Phase W3: packaging, tray, autostart, updater

- Tray icon + menu (status, start-with-Windows, quit) and autostart (HKCU Run).
- Single-instance handling.
- NSIS installer (or Tauri's bundler) + code-signing. No signing keys in the repo.
- Updater (Tauri updater plugin) with channel separation, hash verification,
  rollback. Preserve user data on update/uninstall.

Exit: clean install, update, and uninstall on a fresh Windows machine, data preserved.

### Phase W4: Windows hardening + compatibility

- Fault injection: sleep/wake, device change, engine crash, low disk/memory, corrupt
  settings, duplicate instance, stuck session.
- UIPI (elevated target windows) and RDP-session handling: detect and surface, do not
  silently fail.
- Compatibility matrix: CPU-only laptops, NVIDIA/AMD/Intel, multiple display scales,
  representative target apps (Notepad, Word, Outlook, browsers, terminals, password
  fields).
- Native UAT on physical hotkey + live mic + real focus changes.

Exit: a published minimum/recommended requirement grounded in measured cells.

---

## 4. Measured learnings to carry (from the reference Windows port)

These are the reference's real measurements. They predict Teletype's Windows behavior;
re-measure on Teletype before relying on them.

- **Thread pinning is a production requirement, not a tuning nice-to-have.** On a
  hybrid-core chip (i9-14900KF, 8P+16E), ONNX Runtime's default (all 32 logical
  cores) was 7-10x SLOWER than pinning `intra_op_num_threads` to 6-8 with
  `inter_op=1`. E-core oversubscription makes more threads *worse*. Teletype's
  Windows ASR path is sherpa-onnx (ONNX Runtime), so the provider must set its
  thread count explicitly per hardware profile; the same applies to the
  whisper.cpp path on macOS.
- **The per-frame decode loop is a porting hazard on any high-overhead accelerator.**
  Parakeet's TDT decoder is invoked once per encoder frame (125x for a 10s clip).
  On DirectML each call cost ~171ms of command-list/sync overhead (12.4s for a 10s
  clip, worse than real time). CUDA with an int8 QDQ graph injected 742 Memcpy nodes
  (4.8s for 10s). A QDQ-free fp32 pack dropped that to 0.119s. Consequence: the
  *graph pack* is a first-class porting concern; one model, multiple graph flavors.
- **Model residency favors Windows.** No launchd-style reaper on Windows, so a tray
  app can keep the ASR session and llama-server resident at zero OS cost. The
  reference's decision: resident-by-default in v1, no unload policy. (Teletype
  already keeps llama-server as a subprocess; apply the same resident default.)
- **The Win32 clipboard holds one item, not the macOS multi-item pasteboard.**
  Restoring can only preserve all *formats of the current item*, not all items. A
  user with 3 images copied loses 2. The common case (text) is lossless. This is a
  product decision, not an engineering detail.
- **`SendInput` into an elevated window is blocked by UIPI with no prompt** (silent
  failure). Detect target elevation and surface it. RDP: input injected into the
  console session is invisible to an RDP user; the app must run in the user's session.
- **Timing constants were tuned on Apple Silicon** (50ms poll, 1s caps, 200ms
  post-paste wait, 100ms repair deadline). Re-tune on Windows; do not port the numbers.
- **No TCC permission model, but diagnostics must read per-app UIA capability**, not a
  global grant, because the pattern is always "available" but the app may not implement
  it.
- **Cross-compiling C deps (whisper.cpp/ring) for Windows from a macOS host does not
  work** (needs a native MSVC C toolchain). Build natively on `windows-latest`.
- **`asr.pack`-style config must actually select the files** (the reference found its
  pack setting was a no-op that hardcoded the int8 encoder). Verify the config reaches
  the loader.

---

## 5. What Teletype does differently from the reference

- **Language:** Rust/Tauri, not C#/WinUI 3. The reference's C#-specific gotchas (NAudio,
  WinForms NotifyIcon, ONNX Runtime C# 1.22 API, `.NET` UI Automation facade) are
  context, not instructions. Teletype uses `cpal`/`enigo`/`keyring`/Tauri.
- **ASR engine:** on Windows Teletype uses sherpa-onnx (NeMo Parakeet TDT int8,
  statically linked ONNX Runtime), so this is the one place where the reference's
  ONNX Runtime mechanism really does carry over. whisper.cpp and the ggml
  Parakeet (`parakeet-sys`) are macOS-only dependencies and are not built for
  Windows; on macOS Teletype uses whisper.cpp, ggml Parakeet, the Core ML
  FluidAudio bridge, and parakeet.cpp. The reference used ONNX Runtime with a
  hand-ported TDT decode loop; sherpa-onnx owns that decode loop here. The
  thread-pinning and long-clip chunking concerns still apply.
- **License:** Teletype is MIT and open-source-forever (D001). The reference is GPLv3.
  Do not pull in GPL code (the reference's own notes flagged espanso as GPL-incompatible).

---

## 6. Open decisions

- Windows ASR default model: **decided 2026-10-21, in code.** The Windows
  catalog ships the two sherpa-onnx NeMo Parakeet int8 builds, and
  `catalog.rs::pick_engine` recommends `sherpa-parakeet-v3-int8`
  ("Parakeet v3 (sherpa-onnx)", ~465 MB, English + 24 European languages) on
  every Windows host; `sherpa-parakeet-v2-int8` is the English-only alternative.
  Whisper is not an option to confirm: `whisper-rs` is a macOS-only dependency,
  so there is no Whisper row on Windows and the platform's Whisper stub provider
  fails at `load`. Remaining question: whether to add any further Windows
  engine at all (see the CUDA tier question below).
- Does Teletype ship a GPU ASR tier (CUDA) on Windows, or CPU-only in v1? The reference
  showed GPU helps long dictations but is a large native payload and a runtime-validation
  burden.
- Clipboard multi-item degradation: accept in v1 (text lossless) or invest in OLE
  deferred-render multi-item emulation.
- Updater channel separation (stable/founder/beta) and code-signing identity.
