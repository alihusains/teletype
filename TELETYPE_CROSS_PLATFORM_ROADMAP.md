# Teletype — Detailed macOS + Windows Roadmap

**Baseline reviewed:** `PROGRESS.md`, last updated 2026-10-11  
**Product:** Teletype — open-source, local-first voice dictation, text transformation, and eventually safe voice control  
**Primary optimization:** high impact, low effort, quick wins, repeat usage, reliable downloads  
**Platforms:** macOS first-class; Windows x64 as the next fully validated platform; Windows ARM64 and macOS Intel capability parity only after explicit runtime/build validation.

---

## 1. Executive decision

**You have not drifted away from the original product, but Command Mode has advanced ahead of the production foundations it needs.** The current app already has a strong macOS dictation product, a Rust/Tauri architecture, personalization, AutoText, an updater, release automation, and meaningful tests. The next priority is not a rewrite or an immediate expansion into MCP, Composio, a marketplace, and a full agent.

The new order should be:

1. **Prove the release/distribution pipeline that already exists.** Do not rebuild it blindly; verify real signed artifacts and updater behavior.
2. **Make Windows x64 genuinely usable**, starting with the core dictate/insert/settings/history experience. CI compilation alone is not platform validation.
3. **Make the Cactus/Needle/Whistle runtime reproducible and policy-compliant** on every target where it is enabled. Keep a working Whisper/Parakeet fallback.
4. **Put structured tool registration, permissions, confirmation, and audit logs in place before adding more side-effecting tools.**
5. **Ship the quickest user-visible retention features**: learning feedback, voice macros, macro variables, history search.
6. **Ship Voice Edit**, then a focused Developer Mode.
7. Add MCP, multi-step agent execution, integrations, and a marketplace only after the above has passed acceptance gates.

### Hard rule

A capability is **not Done** merely because code exists locally. A feature is Done only when it is reproducible in CI, packaged for its supported target, exercised by an automated test, manually smoke-tested on that OS, and documented with any limitations.

---

## 2. Current-state audit from `PROGRESS.md`

Treat this table as the current baseline; update status only after verifying the implementation and CI artifacts.

| Area | Reported state | Roadmap decision |
|---|---|---|
| Shared Rust/Tauri architecture | Engine enum, common speech-provider abstraction, model capability metadata, runtime/model separation and feature gates are done | Preserve it. Do not start another broad refactor. |
| macOS dictation | Working production app, reported v0.2.6 | Protect the working pipeline; all new features must preserve text-integrity and raw-output fallback. |
| Core/UI tests | 432 core tests, 121 desktop tests, 11 IPC contract tests; frontend typecheck/build clean | Retain current tests and add platform journey coverage. These counts are not a substitute for Windows manual tests. |
| Streaming LLM/pill, transcript splitting, auto-style, personalization feedback | Reported done | Do not rebuild. Validate behavior on representative apps and machines. |
| AutoText, Quick Add, recovery, onboarding, keybinds, Scratchpad, insights, updater | Reported built | Treat as existing; test their portability and release/update behavior on both systems. |
| Whistle ASR | Provider, FFI, model lifecycle and download/checksum path implemented locally; runtime assets are gitignored; Cactus feature is absent from ordinary dev/release builds | High-priority reproducible-build task. Keep Whisper/Parakeet fallback. |
| Needle Command Mode | Local string parser works; model fallback only works when Cactus runtime is compiled in; current build uses one tool call | Freeze feature expansion until runtime, tool registry, permissions, confirmations and tests are ready. |
| Native action tools | open app/path exists; writing, clipboard, hotkey and file mutation tools are incomplete | Add only after registry and permissions. Read-only tools first. |
| MCP | No client yet | Defer until native tool registry and permissions are implemented. Start with stdio only. |
| Tool registry | Hardcoded schema, no shared metadata | Implement before adding more tools. |
| Permissions / confirmation | Dangerous verb and argument checks exist, but no structured policy and no confirmation UX | Critical safety gap for automation. |
| Agent loop | No multi-step execution/result feedback loop | Defer until tool-call handling is safe, bounded and observable. |
| Windows | Release workflow reportedly has Windows build, but no local Windows development/manual validation; Whistle Windows integration not started | Make Windows x64 a dedicated milestone with real smoke tests. |
| macOS Intel | App build target exists, but upstream Cactus platform list currently documents macOS Apple Silicon, not macOS Intel | Ship core speech with a supported fallback if needed; do not advertise Needle/Whistle parity on Intel until a compatible binary is built and tested. |
| Signing/release status | Progress says `release.yml` covers signing/notarization and updater manifest; historical ADR says unsigned builds shipped | Audit actual latest release and workflow. Do not presume all artifacts are signed/notarized just because YAML has a step. |
| Privacy policy | Teletype promises no telemetry by default; current upstream Needle README says telemetry is enabled by default in its binary and describes `NEEDLE_TELEMETRY=0` and `DO_NOT_TRACK=1` | **Release blocker for Cactus integration until the exact C/FFI runtime path is verified to disable telemetry and network egress is tested.** |

### Important upstream runtime finding

The current Needle device-support guide documents native runtime folders for **Windows x64 and Windows ARM64**, and for **macOS Apple Silicon**. The list does not document a macOS Intel folder. That means Windows support may be an integration/build-packaging task rather than a from-scratch port, but exact ABI/toolchain compatibility still needs a spike. The upstream repository also states telemetry is enabled by default in its binary, so the environment flags must be verified with Teletype's actual statically linked or subprocess runtime—not just assumed to work.

References:
- Needle repository and privacy/telemetry note: https://github.com/cactus-compute/needle
- Supported platform folders: https://cactuscompute.com/blog/needle-supported-devices

---

## 3. Product and architecture guardrails

### Product promise

**Teletype is a private, local-first voice productivity layer that learns the user's vocabulary and workflows.** Dictation remains the dependable default. Voice Edit and Command Mode are explicit modes, not hidden interpretations of ordinary speech.

### Non-negotiables

- No telemetry by default. Do not ship a runtime that sends telemetry until this behavior is understood, disabled and verified.
- Local ASR and local polish remain viable without API keys or accounts.
- Any failed optional transform falls back to safe text; never lose the original transcript because a model failed.
- No shell-string execution from model-generated content. Use typed arguments and OS APIs.
- Model and runtime downloads require HTTPS, pinned versions, SHA-256 verification, atomic install and clear failure/retry states.
- Tool calls are untrusted proposals. A model does not grant itself permission.
- The user's explicit mode/selection overrides inferred intent.
- Default Command Mode is limited and understandable; no hidden sends, deletion, payment, credential use, or privileged actions.
- Keep models and heavyweight runtimes separate from the base app installer where that preserves fast installation and manageable bundle size.
- No full agent loop, MCP ecosystem, Composio or marketplace until native tools have risk metadata and confirmation policy.

### Target architecture

```text
React / Tauri UI
  ├─ Dictate mode
  ├─ Voice Edit mode
  ├─ Command mode
  ├─ Agent mode (later; disabled until safety gates pass)
  ├─ model / permission / onboarding / history UI
  └─ Tauri IPC: validated typed requests only

Shared Rust core (platform-independent)
  ├─ speech-provider trait + model catalog
  ├─ dictation / transform pipeline
  ├─ normalizers + text-integrity validators
  ├─ context model + preference resolution
  ├─ AutoText/macros + closed-set placeholders
  ├─ local history/search + insights
  ├─ command intent + schemas
  ├─ ToolDefinition registry + permission engine
  ├─ bounded executor + confirmation + audit events
  └─ provider/runtime lifecycle and capability negotiation

Platform adapters (macOS / Windows)
  ├─ audio input and device notifications
  ├─ global hotkeys
  ├─ foreground app / focused field / selected text
  ├─ text insertion + verified clipboard fallback
  ├─ open app/file/URL through native OS APIs
  ├─ credential store
  ├─ tray / pill / notifications / permissions
  └─ installer / signature / updater integration

Inference runtimes (independent capability flags)
  ├─ Whisper / Parakeet (established fallback path)
  ├─ Whistle (only where runtime is reproducibly available)
  ├─ local polish provider(s)
  └─ Needle (only where runtime is reproducibly available)
```

**Implementation rule:** keep common command/IPC contracts in Rust, then isolate platform differences behind traits such as `AudioCapture`, `ForegroundContext`, `TextInjector`, `AppLauncher`, `CredentialStore`, and `PlatformPermission`. Avoid `cfg` branching spread throughout the product logic.

---

## 4. Platform technology choices

Prefer the current repository's working crate when one is already stable. Do not replace working macOS code just to standardize names. This table is the recommended implementation target for missing work.

| Capability | macOS implementation | Windows implementation | Shared policy |
|---|---|---|---|
| Main app/UI | Tauri v2 + existing React/TypeScript UI | Tauri v2 + same UI | Same routes, IPC schemas, event names and preference model. |
| Build tooling | Xcode Command Line Tools, Rust toolchain, Node package manager | Visual Studio Build Tools with **Desktop development with C++**, Rust MSVC target, Node package manager, WebView2 runtime for testing | Pin Rust, Node/package-manager and Tauri versions in project docs/CI. |
| Audio capture | Existing audio stack; CoreAudio backend (CPAL is appropriate if that is the current capture abstraction) | Existing audio stack; WASAPI backend (CPAL supports WASAPI) | Keep one `AudioCapture` contract. Normalize to the sample format expected by each ASR provider; explicitly test device changes and permission denial. |
| Global shortcut | Tauri global-shortcut plugin or existing stable implementation | Tauri global-shortcut plugin | Same shortcut configuration format; map Command on macOS and Control on Windows; detect collisions and allow reassignment. The Tauri plugin documents macOS and Windows support: https://v2.tauri.app/plugin/global-shortcut/ |
| Focused app / accessibility | Accessibility API: `AXUIElement`/ApplicationServices; resolve frontmost app with AppKit/NSWorkspace | Windows UI Automation (UIA) through the Windows API / Rust `windows` bindings; foreground HWND/process APIs | Best-effort context only. Every context item records source, timestamp and confidence; missing UIA/AX data must degrade gracefully. |
| Text insertion | Existing AX set-value + read-back verification, then clipboard/paste fallback | UIA Value/Text pattern where supported, then clipboard + Ctrl+V; use `SendInput` only as a controlled fallback | Capture target identity before inference; verify focus/target still matches before inserting; cancel if the user changes target. Preserve original transcript on failure. |
| Clipboard | NSPasteboard or current stable clipboard crate | Win32 Clipboard / current stable cross-platform crate | Do not leave user clipboard overwritten. Restore conditionally only if the clipboard still contains Teletype's payload; do not restore over newer user content. |
| Open app/file/URL | LaunchServices / `NSWorkspace` APIs; `open` may be fallback but never build shell strings | ShellExecuteEx / standard Windows APIs; `Start-Process` is for developer scripts, not app runtime | Validate paths and schemes; no arbitrary shell command execution. Ask before opening ambiguous/sensitive paths. |
| Credential storage | macOS Keychain via existing `secrets.rs` abstraction | Windows Credential Manager through `keyring` or Windows API | Never store API keys in JSON settings, logs, crash dumps or history. |
| Tray / pill / overlays | Tauri windows, always-on-top pill, macOS menu bar/tray APIs | Tauri windows, system tray, Windows foreground/focus rules | Must not steal focus while recording or destroy current target. Test DPI scaling, display changes, sleep/resume and multiple monitors. |
| Model process management | Existing lifecycle manager; use subprocess isolation if native runtime can conflict/crash or blocks capture | Same design, Windows Job Object or child-process lifecycle if using a subprocess | Kill/cleanly stop on quit; no orphan process; handle model-switch races; never starve ASR because an optional command model is loading. |
| Native Needle/Whistle runtime | Upstream documents `macos-arm64`; validate whether Intel build is feasible. Keep Whisper/Parakeet fallback on Intel until proven. | Upstream documents `windows-x86_64` and `windows-arm64`; prioritize x64 and verify that the library artifact links under the project's MSVC toolchain | Pin upstream version/commit, hash files, record provenance/license, verify telemetry-off mode and test with networking blocked. Do not depend on gitignored workstation files. |
| App signing | Apple Developer ID Application, Hardened Runtime, notarization, staple direct-download DMG/app | Authenticode through accepted open-source signing (apply to SignPath Foundation) or alternative trusted signer | Signing is separate from Tauri updater signing. Test actual downloaded artifact from GitHub, not only build outputs. |
| Installer | `.dmg` direct download; optionally Homebrew Cask after release maturity | NSIS `-setup.exe` first; add MSI/MSIX only when a concrete channel/customer needs it | Small installer; download models on demand. Tauri Windows installer docs: https://v2.tauri.app/distribute/windows-installer/ |
| Updates | Existing Tauri updater; signed manifest/artifact, restart flow verified on real macOS install | Existing Tauri updater; verify installer handoff/relaunch behavior on real Windows install | Keep the Tauri updater private key backed up securely and distinct from platform code-signing credentials. Never rotate it casually: installed clients need matching signatures. https://v2.tauri.app/plugin/updater/ |
| CI runner | GitHub-hosted macOS runner for signed build/notarization and architecture-specific tests | GitHub-hosted Windows runner for MSVC build, installer, signing and smoke checks | CI is necessary but not sufficient. Keep physical-device/manual release checklist for audio, permissions, accessibility and injection. |

### Architecture support recommendation

- **macOS ARM64:** full target for core dictation and whichever Cactus runtime passes the gate.
- **macOS Intel:** retain core application/Whisper or Parakeet if already supported; do not advertise Needle/Whistle capability until an x86_64 runtime is actually built and tested. If universal distribution cannot include the runtime on Intel, either package different capability manifests by architecture or ship separate architecture assets.
- **Windows x64:** first Windows target. Validate full core dictation before enabling all Command Mode features.
- **Windows ARM64:** listed upstream for Needle, but defer full Teletype support until the Tauri/Rust app, ASR dependencies, audio, injection and installer are all built and manually tested on ARM64 hardware or reliable test devices.

Tauri's Windows prerequisites include Microsoft C++ Build Tools and WebView2; NSIS and MSI are the standard installer paths. References: https://v2.tauri.app/start/prerequisites/ and https://v2.tauri.app/distribute/windows-installer/.

---

## 5. Priority model

- **P0 / release blocker:** data loss, privacy mismatch, broken distribution, platform build not reproducible, dangerous action executes without permission.
- **P1 / next:** meaningful experience improvements using existing foundations; high impact with limited new architecture.
- **P2 / after a stable release:** expanded automation, MCP and more sophisticated personalization.
- **P3 / later:** agent mode, marketplace, Composio, meetings, diarization, broad integrations.

Effort labels: **S** small, **M** medium, **L** large. They are relative engineering effort labels, not time promises. Sequence by prerequisites rather than treating them as calendar commitments.

---

# PHASE 0 — AUDIT, PIN, AND FREEZE THE CONTRACT

**Priority:** P0  
**Purpose:** Avoid duplicating work, find release blockers, establish a trustworthy platform baseline.

## 0.1 Reconcile the release pipeline status — S

- [ ] Read the current `.github/workflows/ci.yml` and `release.yml` and map every job to an actual output.
- [ ] Inspect the latest published macOS artifacts, not just logs from a test build.
- [ ] Verify Developer ID identity, hardened runtime, notarization submission result and stapling for macOS downloads.
- [ ] Verify Windows Authenticode signature and timestamp for the actual installer and executable; confirm SignPath eligibility and acceptance before relying on it.
- [ ] Verify Tauri updater manifest contains correct OS/architecture entries and a matching signature.
- [ ] Run the updater from the currently published app to the candidate version on clean test machines.
- [ ] Resolve discrepancies between “signed/notarized in CI” and historical “unsigned builds ship” notes. Replace assumptions with artifact-level evidence.

**Acceptance criteria**

- A downloaded macOS artifact passes `codesign --verify --deep --strict --verbose=2`, `spctl --assess --type execute -vv`, and Gatekeeper launch testing. Notarization status is verified through Apple's tooling/logs.
- A downloaded Windows artifact passes `signtool verify /pa /v <file>` or the corresponding signing-provider verification process.
- An existing installation can update to the candidate build, restart, and preserve settings/history.
- Documentation accurately states which architecture and engine combinations are supported.

## 0.2 Establish support matrix — S

Create `docs/platform-support.md` with rows for:

- macOS ARM64: dictation engines, polish providers, Needle, Whistle, updates, signing.
- macOS Intel: same capability breakdown; mark unsupported runtime features clearly.
- Windows x64: same breakdown.
- Windows ARM64: experimental/not supported until tested.

Do not use a single global `supported=true` when support varies by engine or architecture. Drive UI model availability from capability metadata plus actual platform availability.

## 0.3 Pin all native runtime dependencies — M

- [ ] Record Cactus/Needle/Whistle release or commit, source URL, license, API/header version and checksums.
- [ ] Record how `libneedle.a`, `needle.h`, `.cact` models and any runtime binaries are obtained.
- [ ] Remove dependency on local, gitignored files being present on the release machine.
- [ ] Add a documented script or reproducible CI step that fetches/builds the approved artifacts.
- [ ] Fail the build with a clear message when a required feature is enabled but the runtime is missing.
- [ ] Add SBOM/provenance or at minimum a third-party notices entry and SHA-256 manifest.

## 0.4 Privacy gate for Cactus — M, release blocker for the provider

- [ ] Set both `NEEDLE_TELEMETRY=0` and `DO_NOT_TRACK=1` in the runtime environment where the upstream documentation says to do so.
- [ ] Verify those flags affect the exact C API/static-library/subprocess build used by Teletype.
- [ ] Add a test that starts the shipped runtime with outbound network disabled and verifies local transcription/tool-calling still works.
- [ ] Inspect network connections during runtime initialization, inference, model download and shutdown.
- [ ] Distinguish explicit model downloads from telemetry; document that model retrieval is a network operation but inference should be local.
- [ ] If telemetry cannot be reliably disabled in a shipped runtime, do not include that runtime in the default build; keep a compliant fallback or remove the runtime integration.

**Acceptance:** Teletype's “no telemetry by default” promise is true for every process it launches, and this is regression-tested.

## 0.5 Freeze shared contracts — M

Write and test versioned contracts for:

- `SpeechProvider` / engine capabilities.
- `AudioCapture` lifecycle.
- `TextInjector` result states (`inserted`, `target_changed`, `unsupported`, `permission_denied`, `failed`).
- `ForegroundContext` with timestamp and confidence.
- `ToolDefinition`, `ToolRequest`, `ToolResult`, `RiskLevel`, `PermissionDecision`.
- App events: recording state, interim transcript, final transcript, model status, insertion status, command status, confirmation request, recovery available.

All Tauri IPC commands must validate inputs and return typed errors. UI strings are not an authorization mechanism.

**Exit gate:** Phase 0 is done when every dependency/artifact has a reproducible source, platform capabilities are accurately documented, and the privacy and signing gaps are resolved or explicitly block the affected capability.

---

# PHASE 1 — WINDOWS X64 CORE PRODUCT

**Priority:** P0  
**Primary objective:** Make Windows a supported product, not a compile target. Keep macOS stable throughout.

## 1.1 Set up a real Windows development/test environment — S

Recommended environment:

- Windows 11 x64 physical machine for microphone, hotkey and UIA testing.
- Visual Studio Build Tools with **Desktop development with C++**.
- Rust MSVC toolchain `x86_64-pc-windows-msvc`.
- Current stable Node.js and the repository's locked package manager.
- WebView2 Runtime.
- CMake/LLVM only when required by native dependencies; pin versions if the build needs them.
- PowerShell scripts for repeatable local build, test and packaging.

Add `docs/windows-dev-setup.md` with exact verified commands from the repository. Do not copy a guessed list of dependency versions; record the versions that work in CI and on the validation machine.

## 1.2 Windows compile baseline — S

- [ ] Run Rust formatting, clippy and core tests on Windows.
- [ ] Run desktop tests and IPC contract tests on Windows.
- [ ] Run `tsc --noEmit` and Vite production build.
- [ ] Build a Tauri debug app and a release installer.
- [ ] Make native dependencies target-specific in `Cargo.toml` using `cfg(target_os = "windows")` and keep macOS-only frameworks behind macOS target configuration.
- [ ] Resolve all `cfg` errors, linker errors, path encoding issues and newline/path separator assumptions.
- [ ] Add a Windows CI job that fails on compilation/test regression, not just frontend success.

**Acceptance:** a clean checkout builds the Windows x64 app without files copied manually from a developer workstation.

## 1.3 Audio capture and lifecycle — M

Recommended Windows backend: WASAPI through the existing Rust audio abstraction (CPAL supports WASAPI if that is what the existing code uses; retain a working current backend unless testing proves it needs replacement).

- [ ] Enumerate default input and available microphones.
- [ ] Record 16 kHz mono input or resample consistently at the provider boundary.
- [ ] Handle microphone permission denied/revoked.
- [ ] Handle no device, device disconnect, Bluetooth transition and default input changing mid-recording.
- [ ] Ensure start/stop and VAD do not deadlock or leave a recording stream active.
- [ ] Test sleep/wake and device changes.
- [ ] Expose a visible microphone/input-device troubleshooting state instead of a silent failure.

**Acceptance:** repeated record/stop cycles work after app restart, sleep/wake and input-device change; an audio failure never results in fabricated text.

## 1.4 Global hotkeys and mode state — S

Use the existing implementation if already cross-platform; otherwise use Tauri's `global-shortcut` plugin, which supports macOS and Windows.

- [ ] Normalize `CommandOrControl` behavior by OS.
- [ ] Configure hotkeys at runtime and persist them.
- [ ] Detect registration collisions.
- [ ] Expose shortcut conflict and permission states.
- [ ] Ensure press/release events are not lost when the pill changes focus.
- [ ] Test all three modes with keyboard layout differences and modifier keys.

**Acceptance:** hold-to-talk can start and stop reliably in Notepad, Chrome, VS Code and PowerShell without the Teletype window having focus.

## 1.5 Foreground application and focused-control adapter — M

Implement `WindowsForegroundContext` behind the shared interface.

Use:

- `GetForegroundWindow` and `GetWindowThreadProcessId` to identify the foreground window/process.
- `GetClassNameW`/window metadata only as supplemental evidence, not as a replacement for UI Automation.
- Windows UI Automation (UIA) for focused element, control type, value/text patterns, selection and caret data where the target app exposes them.
- The `windows` Rust crate/bindings for Win32/UIA API access; use a small dedicated module so COM/threading rules remain isolated.

Tasks:

- [ ] Capture process path/name and window identity.
- [ ] Determine the focused control and whether it is an editable field.
- [ ] Read selected text only when required by an explicitly invoked Voice Edit or command workflow.
- [ ] Mark UIA-derived context as unavailable when a target application does not expose it.
- [ ] Reject search/address fields for automatic prose polish when the current app/field classifier identifies them.
- [ ] Keep context collection minimal; do not collect entire documents or conversations by default.

**Acceptance:** no crash or app stall when UIA is unsupported, slow, blocked or returns incomplete data.

## 1.6 Text insertion adapter — M, P0 because correctness is critical

Implement `WindowsTextInjector` with this preference order:

1. UI Automation Value/Text pattern only when supported and the operation is appropriate.
2. Clipboard + Ctrl+V as fallback, with clipboard safety and target verification.
3. `SendInput` for narrowly controlled keyboard actions, not arbitrary key sequences produced by the model.

`SendInput` is subject to Windows integrity/UIPI restrictions; elevated apps and some protected controls may reject injected input. Teletype must not elevate itself to work around this.

Tasks:

- [ ] Snapshot foreground HWND/process and focused-control identity before inference.
- [ ] Before insertion, confirm the same target still has focus; otherwise show a cancel/stale-target result.
- [ ] Save clipboard contents only as needed and restore conditionally; never overwrite a newer user clipboard payload.
- [ ] Verify insertion by UIA read-back where available, or report a lower-confidence paste result if the target does not expose text.
- [ ] Handle Unicode, emoji, CJK text, CRLF/LF, long text, empty output and clipboard access failure.
- [ ] Test Notepad, Word/Office if available, Chrome text fields, Slack, VS Code, PowerShell and a privileged-window case.
- [ ] Return typed error states to the UI. Do not show “inserted” when it failed.

**Acceptance:** no text is inserted into a different window after the user changes focus; clipboard is not permanently damaged; fallback behavior is clear.

## 1.7 Native Windows actions — S-M

- [ ] Open application through a safe Windows API, validating it is an installed application.
- [ ] Open a file/folder using a canonical validated path and the OS shell API.
- [ ] Open only explicitly allowed URL schemes (`https`, `http` and any deliberate app scheme); reject `javascript:`, `file:` URL ambiguity, protocol tricks and malformed URLs.
- [ ] Do not pass user/model text into `cmd.exe`, PowerShell or another shell.
- [ ] Record action result and error without recording unnecessary sensitive paths/text.

**Acceptance:** paths with Unicode/spaces, missing files, URLs and uninstalled app names are handled safely.

## 1.8 Windows preferences, secrets and local storage — S-M

- [ ] Keep existing settings/history schema and migrations shared with macOS.
- [ ] Use Windows Credential Manager or a verified `keyring` backend for provider credentials.
- [ ] Verify API secrets never appear in `settings.json`, logs, crash reports or history exports.
- [ ] Use per-user app-data folders; do not require administrator privileges for normal installation/use.
- [ ] Handle path migration, locked files, corrupted JSON and interrupted writes atomically.
- [ ] Make model downloads use `.part` + SHA-256 + atomic rename.

**Acceptance:** install/update/uninstall/reinstall preserves or removes user data according to a documented rule, with no permission elevation needed.

## 1.9 Windows installer and release — M

Use NSIS `.exe` installer first because it is a familiar direct-download path. Add MSI/MSIX later only when justified by user demand or managed-deployment requirements. Tauri's Windows installer uses NSIS or MSI; its WebView2 installer mode must be selected deliberately.

- [ ] Decide the WebView2 bootstrapper strategy: default network bootstrapper is smaller; bundled offline installer is much larger. For an ordinary consumer app, start with default bootstrapper and clearly handle offline failure.
- [ ] Include correct product name, publisher, icon, version, uninstall metadata and protocol registration only where required.
- [ ] Sign installer and executable with an approved service; if using SignPath Foundation, verify the application meets their current OSS eligibility rules and accept that publisher display can be the foundation's identity.
- [ ] Verify Authenticode signature and timestamp after packaging.
- [ ] Test install without admin rights, app launch, update, downgrade/failed update recovery, uninstall and clean reinstall.
- [ ] Publish architecture-labeled release assets and SHA-256 checksums.

**Acceptance:** clean Windows 11 x64 machine can download the installer from GitHub, install, dictate into another app, update and uninstall without developer tools.

## 1.10 Windows manual test matrix — M

Required before claiming Windows stable:

- [ ] Fresh profile/new Windows user.
- [ ] Microphone permission allowed and denied.
- [ ] Default mic and USB/Bluetooth mic.
- [ ] Hotkey collision and user remapping.
- [ ] Hold-to-talk, hands-free/VAD, cancel and recovery.
- [ ] Notepad, browser input, VS Code, Office/Word if available, Slack, PowerShell.
- [ ] Clipboard prefilled with text; clipboard changed while recording; paste failure.
- [ ] Target app changed during inference.
- [ ] Screen scaling 100%, 125%, 150%; one and multiple displays.
- [ ] Sleep/resume, app restart, forced termination and recovery spool.
- [ ] Model missing, partial download, invalid checksum, out-of-disk-space and model-load failure.
- [ ] Installer, updater, rollback/recovery and uninstall.

**Phase 1 exit:** Windows x64 Dictate is stable with Whisper/Parakeet fallback, signed release artifacts verified, and all critical smoke tests passing. Cactus/Needle is not a prerequisite for a useful first Windows release.

---

# PHASE 2 — REPRODUCIBLE CACTUS / NEEDLE / WHISTLE SUPPORT

**Priority:** P0 for enabling those features; P1 if standard dictation already ships successfully without them.

## 2.1 Native artifact compatibility spike — S-M

Build a proof that answers these questions before changing the release workflow:

- Does upstream `windows-x86_64` `libneedle.a` link with the project's Windows MSVC target, or does it require a GNU/MinGW toolchain?
- Is a Windows-compatible `.lib`/COFF artifact available, or must it be built/converted from source?
- What are the exact C ABI calling conventions, symbol exports and required transitive native libraries?
- Does the static library run in process without symbol or runtime conflicts with whisper.cpp/other dependencies?
- Is subprocess execution of the supplied `needle.exe` a safer first integration than FFI?
- Can `NEEDLE_TELEMETRY=0` and `DO_NOT_TRACK=1` disable telemetry in the exact runtime path used?
- Which model format/license applies to each `.cact` file, separately from the engine library's Apache-2.0 code license?

Deliverable: `docs/cactus-runtime-decision.md` with a recommendation (static FFI vs subprocess), exact artifact sources/hashes, build target and privacy test evidence.

## 2.2 Keep runtime boundaries behind one provider — M

- [ ] Keep `WhistleProvider` / command-inference provider behind traits.
- [ ] Add capability discovery per platform/model: language support, streaming, timestamps, keyword bias, maximum audio length, tool calling, confidence.
- [ ] Don't assume any engine supports a capability because the shared provider trait has a method; return an explicit unsupported capability.
- [ ] Separate ASR model lifecycle from command model lifecycle. Command inference must not interrupt an active recording.
- [ ] If the native C API is process-global or can hang/crash, isolate it in a child process and supervise it; if using in-process FFI, add a crash/timeout analysis and prove it won't destabilize dictation.
- [ ] Use a single-flight model switch with cancellation and kill-before-spawn, where applicable.
- [ ] Always cap inference duration and output size; never wait indefinitely.

## 2.3 macOS ARM64 runtime package — M

- [ ] Fetch/build the exact approved runtime from pinned upstream release/commit in CI.
- [ ] Verify header/library compatibility and minimum macOS target.
- [ ] If linked statically, ensure it is included only for the target architecture and that all notices are shipped.
- [ ] If subprocess-based, install the runner in a versioned app-support/runtime directory and validate executable permissions.
- [ ] Download `.cact` models on demand with SHA-256 verification; do not bundle weights in the base installer unless product metrics justify the size.
- [ ] Set privacy/telemetry-off environment at process creation and test with networking blocked.
- [ ] Test ASR and Needle simultaneously, model reload, shutdown, repeated launches, sleep/wake and out-of-memory fallback.

## 2.4 Windows x64 runtime package — M

- [ ] Pin and obtain the upstream Windows x64 engine/build artifacts.
- [ ] Validate MSVC/MinGW ABI compatibility and static/dynamic dependency requirements.
- [ ] If FFI linking creates excessive build risk, use a supervised child-process adapter first; retain a typed request/response protocol and timeout/cancellation.
- [ ] Put the runtime/model under per-user application support, not beside mutable files in Program Files.
- [ ] Verify signatures/hashes for binaries and models before enabling them.
- [ ] Set telemetry-off environment and run offline/network-blocked tests.
- [ ] Verify memory use, latency and shutdown on a mid-range x64 PC—not only a developer's high-end machine.

## 2.5 macOS Intel strategy — S-M

The public Needle supported-device guide documents macOS Apple Silicon but not macOS Intel. Therefore:

- [ ] Do not block the Intel app's core dictation release waiting for a Cactus runtime.
- [ ] Keep Whisper/Parakeet as fallback and mark unavailable provider capabilities in the model catalog.
- [ ] Investigate an x86_64-compatible native runtime only if demand justifies it.
- [ ] If universal binaries are produced, validate each slice with `lipo -info` and run on actual Intel hardware/runner where available.
- [ ] Do not advertise Whistle/Needle as supported on Intel merely because the Tauri app itself is universal.

## 2.6 Runtime eval and performance harness — M

Build `evals/` into a repeatable suite with fixed and permissioned test data:

- Representative everyday dictation, technical speech, names, dates, money, punctuation, noisy mic and silence.
- Localized fixtures only for languages the selected model actually supports.
- Word error rate and a separate “meaning/text integrity” review for normalization/polish.
- Cold load, warm first token, total latency, real-time factor, peak RAM, CPU load and model download size.
- Compare Whisper, Parakeet and Whistle on the same audio fixture set, across macOS ARM64 and Windows x64.
- Compare Needle intent/tool exact-match accuracy on a labelled command suite.
- Store benchmark metadata: OS, CPU, model version/hash, app commit, settings, temperature/prompt config.

**Do not promote Whistle because a vendor benchmark looks better.** Adopt it only if it wins on Teletype's own test data and target hardware without harming reliability.

**Phase 2 exit:** runtime provenance and privacy are verified; CI packages each supported target; model failures degrade safely; benchmarks support the provider decision.

---

# PHASE 3 — COMMON RELIABILITY AND QUALITY GATES

**Priority:** P0. These are product foundations, not optional polish.

## 3.1 End-to-end dictation journey tests — M

Add automated journey tests and OS-specific smoke tests for:

1. Launch → onboarding → grant microphone permission.
2. Download/select model → wait for ready state.
3. Hotkey press → capture → stop → transcribe.
4. Normalize → optional polish → insertion.
5. Reject stale target when focus changes.
6. Handle empty/silence/low-confidence output.
7. Recover interrupted dictation after crash.
8. History/insights update after successful dictation.
9. Model/provider failure falls back to raw transcript.
10. Upgrade from previous version preserves user data.

## 3.2 Error taxonomy and diagnostics — M

Normalize internal error states:

- microphone permission denied
- no input device
- recording start/stop failure
- model missing/corrupt/load failed
- inference timeout/crash
- empty/invalid transcript
- transform failed/invalid output
- injection unsupported/target changed/permission denied
- update/signature failure
- low disk space
- native runtime unavailable/unsupported architecture

Each error should have: stable internal code, safe message, suggested action, retryability, and whether raw-text fallback is available. Do not log audio, full transcripts, selected text, clipboard contents or secrets by default.

## 3.3 Lifecycle/race-condition hardening — M

Complete the deferred risks recorded in project progress:

- [ ] orphaned llama/server/native runner on exit
- [ ] inference stream ends unexpectedly
- [ ] model selection racing provider initialization
- [ ] interrupted model download and cleanup
- [ ] concurrent settings read-modify-write
- [ ] duplicate/stale insertion events
- [ ] cancel during inference/download
- [ ] shutdown while recording or while a command is running
- [ ] stale pill/overlay after display change or sleep/wake

## 3.4 Text-integrity contract — M

- [ ] Original normalized transcript remains available until insertion succeeds.
- [ ] All transforms have size/time limits.
- [ ] Validator catches empty output, truncation, wrong language/drift where detectable, duplicated joins, placeholder corruption and suspicious large rewrites.
- [ ] On validation failure, offer original text or fall back to it automatically according to mode.
- [ ] Explicitly test punctuation, emoji, code, URLs, JSON, Markdown, multi-byte scripts and very long transcripts.

## 3.5 Baseline performance budgets — S-M

Collect first; set numeric targets from real baseline rather than guessing:

- app cold/warm launch
- hotkey-to-recording start
- recording-start UI latency
- time from stop to first output
- time to final insertion
- ASR time / audio duration
- local polish p50/p95
- memory at idle, recording and local inference
- peak memory while ASR and command model coexist
- installer and first-model download size

Store in `docs/benchmarks.md`. Treat a material regression as a CI/release review item.

---

# PHASE 4 — SAFE COMMAND FOUNDATION (BEFORE MORE TOOLS)

**Priority:** P0 before expanding automation. The current app has basic parsing/validation but lacks a structured permission engine, confirmation UX and registry.

## 4.1 Tool registry — M

Replace hardcoded tool JSON as the source of truth with a shared `ToolDefinition` structure:

```rust
struct ToolDefinition {
    id: String,
    name: String,
    description: String,
    input_schema: JsonSchema,
    source: ToolSource,          // native | mcp | integration
    risk_level: RiskLevel,       // read | low_risk | write | destructive | privileged
    requires_confirmation: bool,
    enabled_by_default: bool,
    supported_platforms: Vec<Platform>,
    timeout_ms: u64,
    output_limit_bytes: usize,
    audit_category: String,
}
```

Implementation notes:

- Use a stable tool ID separate from the spoken name and model-visible name.
- JSON schema validation is mandatory before invocation.
- Platform availability is explicit, not implied by build target.
- Tool arguments use typed fields; reject unknown fields when practical.
- Model receives only tools relevant to the current intent/context, not every available tool.
- Result payload is size-limited and treated as untrusted data.
- Registry metadata is consumed by UI, permission engine, audit logs and tests.

**Acceptance:** every registered tool appears in a registry contract test; no production tool exists only in a separately maintained hardcoded JSON schema.

## 4.2 Permission engine — M, P0

Implement central policy:

| Risk | Examples | Default behavior |
|---|---|---|
| READ | get current app, list allowed app names, read specifically requested selected text | Execute only when explicitly requested and scoped; minimize sensitive data. |
| LOW_RISK | open known application; open validated URL | Execute after clear user intent; display result. Confirmation can be a per-tool setting. |
| WRITE | type text into a target field; create a file; write clipboard | Require confirmation by default until reliability and UX are proven. Re-check target immediately before action. |
| DESTRUCTIVE | delete, overwrite, move/rename bulk files, close unsaved work, send/publish messages | Mandatory confirmation with action summary and exact target; never execute from model confidence alone. Keep disabled by default in early releases. |
| PRIVILEGED | administrator operations, shell/script execution, credential access, security settings | Disabled in Command/Agent Mode. No broad privilege escalation. |

- [ ] All tool invocations pass through one policy decision point.
- [ ] Model output cannot set or increase its own risk/permission.
- [ ] Confirmations show action, target, data to be changed and whether anything leaves the device.
- [ ] Bind confirmation to a specific request ID, arguments hash and expiry; reject stale/replayed confirmations.
- [ ] Cancel is always available and cancellation propagates to the executor.
- [ ] Denial does not silently retry with a different tool.

## 4.3 Confirmation UX — S-M

Use the existing pill/command panel:

- Clear verb-object summary (“Create `notes.md` in Documents?”).
- Show exact path, app, URL or target text.
- Buttons: Confirm / Cancel; no preselected destructive choice.
- Visible state: preparing, awaiting confirmation, running, complete, failed/cancelled.
- If focus/target changes, invalidate confirmation and re-ask.
- For a text change, show a preview/diff where practical.
- Do not show the full confirmation on harmless read-only actions unless needed.

## 4.4 Structured audit events — M

Record locally, with privacy minimization:

- request/action ID
- timestamp
- tool ID/version
- risk level and decision (allowed/confirmed/denied)
- platform + target type (not necessarily full title/path)
- duration and outcome/error code
- whether the user cancelled

Do not store credentials, full clipboard payload, audio, full selected documents or complete transcripts in command audit logs. Make history/audit retention visible and clearable.

## 4.5 Native tool implementation order

### Tier A — Read/basic low risk

1. `get_active_application`
2. `list_allowed_applications` / resolve installed app
3. `open_application`
4. `open_validated_url`
5. `open_file` / `open_folder` after validating path and intent

### Tier B — Text productivity

6. `transform_selected_text` (Voice Edit service, not arbitrary app automation)
7. `insert_text` via the verified `TextInjector`
8. `clipboard_write` with preview/confirmation where needed
9. `clipboard_read` only after explicit invocation and a clear privacy notice; never inject clipboard into a prompt by default
10. `create_file` / `create_folder` with path validation and confirmation

### Tier C — Only after security/UX review

11. `switch_application`
12. `press_key` / `hotkey` with strict allowlist of keys/actions
13. `close_application` with unsaved-work risk considered
14. file move/rename/copy

### Do not implement in early Command Mode

- arbitrary shell execution
- arbitrary scripts or command strings
- deletes/bulk destructive operations
- password/keychain access
- sending email/Slack/messages without a final confirmation
- administrator or security-setting changes

**Phase 4 exit:** all side effects use registry + permission engine + confirmation + audit; no model-generated action bypasses this path.

---

# PHASE 5 — QUICK-WIN PRODUCT RETENTION

**Priority:** P1; move ahead of MCP/Agent Mode. This phase uses foundations that are already partly built.

## 5.1 Make learning visible — S

- [ ] Display “learned by Teletype” badge on learned dictionary entries.
- [ ] Show confidence as new/building/strong with explanation; avoid claiming statistical certainty beyond the actual mechanism.
- [ ] Add subtle, dismissible “Teletype learned…” notifications only after a stable new preference is accepted.
- [ ] Add a visible count of fixes/corrections actually applied.
- [ ] Clicking a learning notification opens the exact setting/word, with Undo/Forget.
- [ ] Do not notify for every observation; batch low-value changes and throttle notifications.
- [ ] Never treat arbitrary rewrites as misheard words; preserve existing punctuation/synonym guards and add regressions.

**Acceptance:** a user can see a real learned item, inspect it, correct it, undo it or turn learning off.

## 5.2 Voice Macros — S-M

- [ ] Promote AutoText into a first-class Voice Macros screen.
- [ ] Macro has stable ID, spoken trigger(s), optional typed trigger, template, scope, enabled flag and last-used/count metadata.
- [ ] Support global and per-app scopes.
- [ ] Provide first-run sample macros: standup, PR description, Jira bug, meeting follow-up, email sign-off.
- [ ] Avoid requiring an LLM for deterministic template expansion.
- [ ] Resolve exact macro aliases deterministically before generic polishing only when there is no ambiguity with normal dictation.
- [ ] If a spoken trigger is ambiguous, show a suggestion or require explicit Macro Mode; do not replace normal speech unexpectedly.
- [ ] Add preview, duplicate, edit, enable/disable, delete and import/export.

## 5.3 Closed-set macro placeholders — S

Add deterministic placeholders:

- `{{date}}`
- `{{time}}`
- `{{clipboard}}` only when the user explicitly authorizes clipboard insertion
- `{{app_name}}` only where context is reliably available

Future optional placeholders: `{{selected_text}}` for explicitly invoked workflows and `{{cursor}}` if the insertion API supports it. Never support arbitrary code/eval.

## 5.4 Global local history search — M

- [ ] Search transcript text, date, application and macro metadata.
- [ ] Start with literal/normalized full-text search; add fuzzy search after correctness.
- [ ] Add keyboard shortcut and a small global search palette.
- [ ] Return match context and open the original history item.
- [ ] Keep indexing on-device; avoid sending history to cloud models or embedding services by default.
- [ ] Respect deletion, retention and exclude-list preferences.
- [ ] Make search robust to history corruption and handle large archives without freezing the UI.
- [ ] Consider semantic search only after usage proves keyword search insufficient and a fully local embedding path is verified.

## 5.5 Better Insights — S-M

Use metrics the app actually measures:

- time saved (clearly show that the comparison is an estimate)
- words dictated
- actual transformations/corrections applied
- learned words/preferences
- most-used macros
- per-app usage totals
- weekly activity

Do not invent peer percentiles (“top 5%”) when Teletype has no valid cohort data. A personal trend or personal best is more honest and privacy-compatible.

## 5.6 Personalization controls — S-M

- [ ] app-scope picker for preferences
- [ ] edit an existing preference
- [ ] group learned preferences by application category
- [ ] one master personalization off switch
- [ ] clear learned data without deleting dictionary entries unless the user asks
- [ ] export/import user profile with schema versioning and preview before import
- [ ] per-app language override UI if still only configurable through JSON

**Phase 5 exit:** new users can see the learning system work, create a macro, reuse it, and find a prior transcript without configuring JSON.

---

# PHASE 6 — VOICE EDIT

**Priority:** P1 / top feature after distribution + core stability. This is a distinct interaction, not ordinary dictation with a different prompt.

## 6.1 Explicit Voice Edit activation — M

- [ ] Add configurable “Edit Selection” shortcut and a visible entry in the pill/menu.
- [ ] Capture selected text through AX/UIA only after explicit activation.
- [ ] Capture a minimal context window only when required and consented; default to selected text alone.
- [ ] Capture the target app/field identity and a hash or version of the selection.
- [ ] If there is no selection, explain how to select text; do not read an entire document silently.
- [ ] Show a distinct edit-mode indicator so commands are not confused with dictated text.

## 6.2 Edit intent parsing — M

Initial deterministic/structured actions:

- grammar/punctuation fix
- rewrite professionally
- make casual
- shorten
- expand
- simplify
- convert to bullets
- summarize selected text
- translate to chosen language
- replace exact phrase

- [ ] Model output is a proposed replacement, never an executable action.
- [ ] Require explicit mode/hotkey for v1; do not depend on free-form global intent detection.
- [ ] Constrain output to the requested operation and selected text.
- [ ] Reuse existing transform provider/fallback where safe.
- [ ] Keep a plain-text fallback and avoid discarding selection if the model fails.

## 6.3 Review, replace and undo — M

- [ ] For short edits, allow direct replacement after user activation; optionally provide a preview setting.
- [ ] For large replacements, show before/after diff and Confirm/Cancel.
- [ ] Recheck foreground app, focused field and selection before replacement.
- [ ] Abort if selection or target has changed.
- [ ] Replace atomically as possible; preserve text if injection verification fails.
- [ ] Add one-step undo/restore of the previous selection when possible.
- [ ] Log only the operation status by default, not both full texts.

## 6.4 Platform requirements

**macOS:** `AXUIElement` selected text/range where supported; AX permission onboarding; text set/readback and clipboard fallback; test native apps and Electron/Chromium apps separately.

**Windows:** UIA TextPattern selection/caret when supported; ValuePattern where appropriate; clipboard/Ctrl+V fallback; test elevated apps as unsupported rather than bypassing OS protection.

**Acceptance test suite:** grammar edit; bullets; shorten; long selection; Unicode/emoji; code block; unsupported field; focus changes mid-inference; cancellation; provider timeout; insertion failure; undo.

---

# PHASE 7 — CONTEXT ENGINE + DEVELOPER MODE

**Priority:** P1 after Voice Edit foundation.

## 7.1 Context resolver — M

Resolve context by confidence and minimal data:

1. explicitly chosen mode/transform
2. active selection/field purpose when explicitly requested
3. foreground app category
4. user preferences for that category
5. global default

- [ ] Make precedence deterministic and tested.
- [ ] Add a small `ContextEvidence` object with source, timestamp and confidence.
- [ ] Don't infer a full page/conversation from app name alone.
- [ ] Don't send full document content to an LLM for app styling.
- [ ] Unknown app/field → global behavior with no destructive transformation.

## 7.2 Terminal/browser preservation — S-M

- [ ] Recognize terminal apps by process/application metadata.
- [ ] Preserve flags, quotes, paths, env vars, commands, URLs and code tokens.
- [ ] Recognize browser address/search fields when UIA/AX data supports it; conservative fallback if field role is unknown.
- [ ] Add tests around `--`, pipes, redirects, brackets, braces, file extensions, ports, URLs and environment variables.
- [ ] Do not auto-polish a command or URL into prose.

## 7.3 Developer Mode — M

Add a mode/profile rather than a separate application. Starter features:

- Developer vocabulary pack: GitHub, Rust, Tauri, TypeScript, React, Docker, Kubernetes, AWS/Azure, SQL, JSON, APIs and common library names.
- Macros: commit message, PR description, Jira bug, release notes, incident update, code comment, technical explanation.
- Technical style: concise, token-preserving, filenames/casing-aware.
- Separate `dictation`, `code-comment`, `terminal` and `prompt` output styles; terminal commands should not be invented from vague speech.
- Custom vocabulary import/export.
- User-controlled aliases for acronyms, proper nouns and project-specific terms.

## 7.4 ITN completion — M

Finish remaining ITN 2b cases from the existing roadmap before expanding to more advanced generation:

- compound ordinals
- cardinals with spoken “and”
- spoken punctuation
- currency/money ordering
- regression fixtures for false positives (for example “second” used as an ordinary noun)

Every rule must have positive and negative fixtures. Keep ITN deterministic and separate from LLM polishing.

**Phase 7 exit:** Teletype preserves computer syntax and produces useful developer-ready text without turning itself into a generic coding agent.

---

# PHASE 8 — COMMAND MODE HARDENING AND LIMITED AGENT LOOP

**Priority:** P2. Do not add multi-step agent execution before Phase 4 is complete.

## 8.1 Make the Cactus/Needle runtime part of production builds — M

- [ ] Compile/include the runtime through pinned reproducible artifacts for macOS ARM64 and Windows x64.
- [ ] Gate model-backed fallback on a runtime capability check, not merely a Cargo feature that may accidentally be missing in a release.
- [ ] Surface runtime status (“available”, “downloading”, “unsupported on this architecture”, “failed”) in UI.
- [ ] Keep the fast deterministic command parser first for unambiguous simple commands if evals show it is reliable; model fallback for ambiguous/unclassified requests.
- [ ] Do not treat the model's confidence as permission.
- [ ] If runtime unavailable, give a clear unsupported-command message or stay in dictation according to the explicitly selected mode.

## 8.2 Intent evaluation set — M

Build a local labelled command corpus that includes:

- direct app/file/URL opens
- text meant to be dictated but containing verbs such as “open”, “send”, “delete” in a sentence
- ambiguous phrases
- unsupported requests
- malicious prompt-injection-like content in selected text or file names
- misspellings, accents, filler words and partial utterances

Measure intent classification precision/recall, false actions and refusals. **False action rate matters more than total command coverage.** Add a regression fixture for every incident.

## 8.3 Bounded execution loop — M

Only after tool registry/permission engine/confirmation work:

- [ ] First release: at most 2 execution/tool-result cycles in Command Mode.
- [ ] `max_steps`, per-tool timeout, total deadline, cancellation token, request ID and duplicate-call guard.
- [ ] Pass only bounded, normalized tool results back to the model.
- [ ] Treat tool output as untrusted content, not instructions.
- [ ] No automatic chain can escalate risk level or bypass confirmation.
- [ ] Re-evaluate permissions for every step.
- [ ] On timeout/crash/ambiguous output, stop and show what completed; do not silently retry write actions.
- [ ] No long-running Agent Mode in the first Command Mode release.

## 8.4 Mode UX — S-M

Three explicit modes eventually:

- **Dictate:** convert speech to text; no app actions.
- **Command:** one narrowly scoped action or text transform with permissions/confirmations.
- **Agent:** multi-step plan and execution, disabled until its safety release gate passes.

UI should always show the current mode, whether the model is thinking, any pending confirmation and the result. Mode-specific shortcuts must be configurable.

**Phase 8 exit:** Command Mode behaves predictably with runtime present/absent; every action is validated, permissioned, cancelable, bounded and audited.

---

# PHASE 9 — MCP CLIENT AND EXTERNAL TOOLS

**Priority:** P3. Implement after the native registry; start local and narrowly scoped.

## 9.1 MCP transport v1 — M-L

- [ ] Start with **stdio MCP** for a single allowlisted local server, such as a read-only filesystem or Git server.
- [ ] Pin executable/package source and version; show exact command and accessible directories.
- [ ] Use explicit server allowlist; no auto-discovery of every process/server on the machine.
- [ ] Treat all server metadata and outputs as untrusted.
- [ ] Validate JSON schema, timeouts, output size and cancellation.
- [ ] Map every MCP tool into `ToolDefinition`; all calls go through the same permission engine and audit system.
- [ ] Disable network MCP transports in v1 unless a clear use case and credential/security review justify them.

## 9.2 MCP permission and UX — M

- [ ] Per-server enable/disable.
- [ ] Per-tool enable/disable.
- [ ] Per-folder scopes for filesystem tools.
- [ ] Confirm writes/destructive operations.
- [ ] Revoke an MCP server and all its permissions in one action.
- [ ] Show which server/tool will be called and what data it will receive.

## 9.3 Integrations

Only after stdio MCP is stable:

1. one read-only Git/GitHub workflow
2. one local filesystem workflow
3. one chosen user-requested integration

Do not start MCP marketplace and Composio in parallel. Cloud integrations may transmit data and conflict with the default local-first story; they must be opt-in, visibly identified, and separately permissioned.

---

# PHASE 10 — EXPANSION FEATURES

These are valuable only once installation, dictation, learning, macros and platform parity are healthy.

## 10.1 Audio/video file transcription — M

- Drag/drop WAV, MP3, M4A and other formats whose decoder is implemented and tested; add common video containers after extracting audio reliably.
- Show progress, cancellation and model choice.
- Add transcript to local history only after success.
- Allow copy/export to Markdown/plain text.
- Never upload files by default.
- Keep a fixed fixture set for long-file/chunk boundaries and re-join correctness.

## 10.2 Language and mixed-language quality — M-L

- Expand by supported engine and real demand, not a headline language count.
- Add explicit language capability metadata per engine/architecture.
- Prioritize language pairs common among Teletype users and technical vocabulary.
- Build code-switching evals before claiming support.
- Allow per-app or per-profile language selection and transparent automatic-detection behavior.

## 10.3 Advanced ASR features — M-L

- Whistle keyword biasing and timestamps if supported by the selected build.
- Streaming ASR only after benchmarks show a product need and it clearly improves stop-to-text perception.
- Speaker diarization only if file/meeting use cases justify added model size and complexity.
- Always keep fallback to established speech engine.

## 10.4 Semantic history — M

- Start with local text index.
- Only add local embeddings/vector search if people use history search enough to demonstrate keyword-search limitations.
- No cloud embeddings by default.
- Include delete/rebuild index, corruption recovery and privacy/retention controls.

---

# PHASE 11 — RELEASE AND SUPPORT EXCELLENCE

## 11.1 CI matrix

**Pull request jobs**

- macOS ARM64 build and tests
- macOS Intel build if that slice remains a supported release target
- Windows x64 MSVC build and tests
- format + clippy
- core / desktop / IPC contract tests
- frontend typecheck + production build
- security-sensitive contract tests for registry/policy/injection
- feature-enabled Cactus job on supported architectures
- dependency/license/provenance scan for native assets

**Release jobs**

- version/tag validation
- clean build from pinned dependencies
- model/runtime hash verification
- platform signing
- macOS notarization and staple verification
- Windows Authenticode verification
- updater artifact/signature/manifest verification
- installer smoke test
- create checksum file and release notes
- publish assets only if every required gate passes

## 11.2 Release acceptance gate

No release unless:

- [ ] no P0 bug is open
- [ ] tests pass on every advertised target
- [ ] fresh install works on clean OS profiles
- [ ] microphone/hotkey/insert journeys pass
- [ ] the latest upgrade path is tested
- [ ] signing/notarization is verified for actual output artifacts
- [ ] model/runtime downloads are verified and recoverable
- [ ] privacy claims match network behavior
- [ ] known architecture limitations are in release notes
- [ ] recovery works after an interrupted recording/update

## 11.3 User feedback without default telemetry

- GitHub Issues/discussions and opt-in feedback.
- Local exportable diagnostics bundle with explicit review before sharing; redact transcripts, paths, titles and identifiers by default.
- Local usage insights only for user benefit.
- If aggregate telemetry is ever considered, it must be an explicit product/maintainer decision, off by default, documented and consented—not introduced by a native runtime dependency.

---

# PHASE 12 — DEFERRED / NOT NOW

Keep these in the long-term backlog, not the next delivery window:

- agent marketplace
- Composio integration
- full MCP ecosystem and network transport by default
- unrestricted computer-control agent
- arbitrary shell/script execution
- destructive file automation
- meeting recorder/Notetaker clone
- speaker diarization
- large new language-count push without mixed-language evals
- full streaming-ASR rewrite without measured evidence
- advanced probabilistic/“Jev-class” decision layer before it enables a validated user experience
- macOS Intel Cactus runtime unless a compatible build can be supported cleanly or user demand merits the cost
- Windows ARM64 first-class support until Windows x64 is fully validated

---

# 6. Concrete sequencing / release milestones

Do not attach hard calendar durations before the first Windows machine and runtime build are tested. Use this dependency order.

## Milestone A — Release baseline verified

1. Reconcile actual signing state of the latest macOS/Windows builds.
2. Document supported OS/architecture/provider matrix.
3. Pin runtime assets and run privacy gate.
4. Ensure updater key is safely backed up and distinct from OS signing keys.

**Ship gate:** actual artifacts pass signing and update tests.

## Milestone B — Windows x64 Dictate beta

1. Native Windows build/dev setup.
2. Audio, hotkey, foreground context, text insertion and clipboard.
3. Whisper/Parakeet as baseline engines.
4. Models/history/settings/onboarding/updater.
5. Windows app-level smoke test matrix.
6. Signed installer.

**Ship gate:** core dictation works without Cactus; no focus-target data loss; clean install/update/uninstall passed.

## Milestone C — Cross-platform runtime integration

1. Build spike for the exact Cactus native artifacts.
2. Telemetry/privacy validation.
3. macOS ARM64 packaging.
4. Windows x64 packaging.
5. Model download, checksum, cancellation and lifecycle tests.
6. ASR and command inference benchmarks on both platforms.

**Ship gate:** runtime is reproducible, safe, privacy-compatible and faster/better enough to justify default availability.

## Milestone D — Command safety

1. Tool registry.
2. Permission engine.
3. Confirmation UX.
4. Audit events.
5. Contract/eval set.
6. Read-only/low-risk native tools.
7. Bounded two-step Command Mode loop.

**Ship gate:** every action is permissioned and recoverable; write actions require confirmation; no model output can bypass safety.

## Milestone E — Retention quick wins

1. Learned badges + notifications.
2. Voice Macros.
3. Closed-set variables.
4. Global history search.
5. Honest personal insights.
6. User control over learning/export/delete.

**Ship gate:** all features are usable without JSON edits, preserve local-only data policy and have UI journey tests.

## Milestone F — Voice Edit + Developer Mode

1. Explicit selection-based mode.
2. Context capture/validation.
3. Transform and diff/preview.
4. Safe replace + undo.
5. Developer macros and vocabulary.
6. Terminal/browser protection.
7. ITN 2b regression suite.

**Ship gate:** no stale-target replacement; users can cancel/undo; code, URLs and commands are preserved.

## Milestone G — MCP / Agent extension

1. MCP stdio client.
2. Map MCP tools into registry.
3. Per-server and per-tool permissions.
4. One read-only server.
5. One low-risk write workflow with confirmation.
6. Agent Mode only after evaluation and red-team suite.

**Ship gate:** permission isolation, timeout/cancel, tool-result prompt-injection tests and audit all pass.

## Milestone H — Discovery and expansion

1. Audio/video drop-zone transcription.
2. Local history export and optionally semantic search.
3. Mixed-language improvements.
4. Whistle/streaming ASR if evals prove value.
5. ARM64/Intel runtime targets based on real demand.

---

# 7. Platform-specific release checklists

## macOS release checklist

- [ ] Build on supported macOS runner/toolchain.
- [ ] Verify ARM64 and Intel capability matrix individually; don't infer runtime parity from universal Tauri binary.
- [ ] Confirm microphone permission prompt and recovery after denial.
- [ ] Confirm Accessibility permission instructions are accurate and target text fields are readable/writable.
- [ ] Test global hotkey, tray/menu, pill overlay and no focus stealing.
- [ ] Verify native child processes exit on app quit and after model switch.
- [ ] Check entitlements are minimum necessary; Hardened Runtime is enabled.
- [ ] Developer ID sign all executable code and nested components.
- [ ] Submit notarization, inspect failures/logs, staple supported distributed artifact.
- [ ] Test Gatekeeper launch after downloading from GitHub on a clean Mac.
- [ ] Test updater from previous published release.
- [ ] Publish DMG, signature/checksum details, release notes and supported architecture table.

Official reference: https://developer.apple.com/developer-id/ and https://v2.tauri.app/distribute/sign/macos/.

## Windows release checklist

- [ ] Build x64 with MSVC on a Windows runner.
- [ ] Confirm WebView2 installation behavior.
- [ ] Confirm mic privacy permissions and global shortcut.
- [ ] Test foreground HWND/UIA capture and clipboard restore.
- [ ] Test text insertion into Notepad, browser, VS Code, Office where available and PowerShell.
- [ ] Confirm app doesn't request administrator privileges for standard use.
- [ ] Confirm any runtime executable/model comes from pinned source with checksum verification.
- [ ] Authenticode-sign installer and executable where applicable; verify signature and timestamp.
- [ ] Test SmartScreen from a clean user profile and document that a valid signature does not guarantee immediate reputation.
- [ ] Test install/update/uninstall and recovery from a failed update.
- [ ] Publish installer, checksums, release notes and architecture support.

Official references: https://v2.tauri.app/start/prerequisites/, https://v2.tauri.app/distribute/windows-installer/, https://v2.tauri.app/distribute/sign/windows/ and https://learn.microsoft.com/en-us/windows/win32/winauto/windows-automation-api-portal.

---

# 8. What to do first: next 10 tickets

1. **T0-01 — Verify latest release artifacts and signing state** (S)
2. **T0-02 — Publish support matrix for macOS ARM64/Intel and Windows x64/ARM64** (S)
3. **T0-03 — Cactus runtime ABI + telemetry-off spike** (S-M)
4. **W1-01 — Windows clean-checkout build/test on MSVC** (S)
5. **W1-02 — Windows audio/hotkey lifecycle test** (M)
6. **W1-03 — Windows TextInjector: focus snapshot, UIA/clipboard fallback, read-back** (M)
7. **W1-04 — Windows x64 end-to-end Dictate smoke matrix** (M)
8. **P4-01 — ToolDefinition registry contract** (M)
9. **P4-02 — Central permission engine + confirmation UX** (M)
10. **R5-01 — Visible learning + Voice Macros design/implementation slice** (S-M)

Do not start MCP, Composio, Tool Marketplace, unrestricted Agent Mode, Windows ARM64 polish, or a full ASR rewrite while these blockers remain.

---

# 9. Definition of done for every feature

A roadmap task is complete only when all relevant checks pass:

- [ ] Product behavior and user-visible outcome are defined.
- [ ] Shared contract is platform-independent where possible.
- [ ] macOS implementation verified or explicitly marked unsupported.
- [ ] Windows implementation verified or explicitly marked unsupported.
- [ ] Unit and contract tests cover happy path, failures, cancellation and boundary cases.
- [ ] Security/privacy requirements are stated and tested.
- [ ] No raw transcript/audio/selected content/secrets enter logs by default.
- [ ] Runtime/model artifacts are pinned and checksum-verified.
- [ ] Real platform smoke test is recorded in CI/release evidence or the validation log.
- [ ] Documentation, support matrix and release notes are updated.
- [ ] A fallback path exists for optional model/provider failure.

---

# 10. Source references to keep with this roadmap

- Teletype project baseline: `PROGRESS.md` (2026-10-11).
- Tauri prerequisites: https://v2.tauri.app/start/prerequisites/
- Tauri GitHub Actions distribution: https://v2.tauri.app/distribute/pipelines/github/
- Tauri global shortcuts: https://v2.tauri.app/plugin/global-shortcut/
- Tauri Windows installers: https://v2.tauri.app/distribute/windows-installer/
- Tauri updater signing: https://v2.tauri.app/plugin/updater/
- Tauri macOS signing: https://v2.tauri.app/distribute/sign/macos/
- Tauri Windows signing: https://v2.tauri.app/distribute/sign/windows/
- Apple Developer ID / notarization: https://developer.apple.com/developer-id/
- Windows UI Automation: https://learn.microsoft.com/en-us/windows/win32/winauto/windows-automation-api-portal
- Windows SendInput limitations: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput
- Cactus/Needle native targets and telemetry: https://github.com/cactus-compute/needle
- Cactus supported device/runtime folders: https://cactuscompute.com/blog/needle-supported-devices
- SignPath Foundation OSS eligibility: https://signpath.org/terms.html

---

## Final strategic recommendation

**Keep the current shared Rust/Tauri core. Do not restart the architecture.** Treat the macOS product as the stable reference implementation, make Windows x64 a rigorously tested second platform, make all native runtimes reproducible and privacy-compliant, and put the tool/permission boundary ahead of agent expansion. Then use the existing foundations to ship the low-effort features users can feel: visible learning, macros, history search, Voice Edit and Developer Mode.

The winning sequence is:

**Reliable installation → dependable dictation → safe command foundation → visible personalization → repeatable macros/history → Voice Edit → Developer Mode → MCP/Agent expansion.**
