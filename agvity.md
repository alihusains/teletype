# Strict QA Code Review & Production Readiness Audit

**Audit Date:** 2026-10-05  
**Target:** Teletype (`com.teletype.app`)  
**Auditor:** Strict QA / Principal Architecture Review  

---

## Executive Summary

A comprehensive code and architecture audit of the Teletype application was conducted. The primary symptom reported—**multiple settings appearing twice in the Settings tab**—is caused by an incomplete refactoring where **10 newly ported settings sections were placed at the top of [`SettingsScreen.tsx`](ui/src/screens/SettingsScreen.tsx), while the legacy inline sections below them were left active**. 

Additionally, the audit uncovered:
1. **A Rust compilation blocker** in [`crates/teletype-core/src/transforms/engine.rs`](crates/teletype-core/src/transforms/engine.rs) preventing `cargo test --workspace` from passing.
2. **A critical IPC casing mismatch** in [`ui/src/screens/TransformsScreen.tsx`](ui/src/screens/TransformsScreen.tsx) that permits users to permanently delete core shipped transforms (e.g. Polish, Professional) and breaks custom transform creation.
3. **An application crash on quit** (`SIGABRT` in `ggml_metal_rsets_free -> ggml_abort` on `NSApplication terminate:`).
4. **Cross-screen state desynchronization** between Settings, AutoText, and Dictionary screens.
5. **Background battery and CPU drain** caused by an unthrottled 33ms animation timer running across hidden screens.

---

## 1. Deep Dive: Settings Tab Duplication Defect

### 1.1 Root Cause Analysis

In recent commits, the settings architecture was modernized by porting settings into dedicated modules located in [`ui/src/settings/`](ui/src/settings/):
* `TranscriptionSection` ([`ui/src/settings/DictationPolishSections.tsx`](ui/src/settings/DictationPolishSections.tsx))
* `LivePreviewSection` ([`ui/src/settings/DictationPolishSections.tsx`](ui/src/settings/DictationPolishSections.tsx))
* `AIPolishSection` ([`ui/src/settings/DictationPolishSections.tsx`](ui/src/settings/DictationPolishSections.tsx))
* `KeybindsSection` ([`ui/src/settings/KeybindsClipboardSections.tsx`](ui/src/settings/KeybindsClipboardSections.tsx))
* `ClipboardSection` ([`ui/src/settings/KeybindsClipboardSections.tsx`](ui/src/settings/KeybindsClipboardSections.tsx))
* `MicrophoneSection` ([`ui/src/settings/MicrophoneSoundsSections.tsx`](ui/src/settings/MicrophoneSoundsSections.tsx))
* `SoundsSection` ([`ui/src/settings/MicrophoneSoundsSections.tsx`](ui/src/settings/MicrophoneSoundsSections.tsx))
* `PermissionsSection` ([`ui/src/settings/PermissionsUpdatesLicenseSections.tsx`](ui/src/settings/PermissionsUpdatesLicenseSections.tsx))
* `UpdatesSection` ([`ui/src/settings/PermissionsUpdatesLicenseSections.tsx`](ui/src/settings/PermissionsUpdatesLicenseSections.tsx))
* `LicenseSection` ([`ui/src/settings/PermissionsUpdatesLicenseSections.tsx`](ui/src/settings/PermissionsUpdatesLicenseSections.tsx))

However, in [`ui/src/screens/SettingsScreen.tsx`](ui/src/screens/SettingsScreen.tsx), the old legacy sections were **not removed**. Both the new sections and the legacy blocks were mounted consecutively in the JSX:

| Setting Field | Render 1 (New Modular Section) | Render 2 (Legacy Block in `SettingsScreen.tsx`) |
| :--- | :--- | :--- |
| **`hotkey`** | `<KeybindsSection>` | "Dictation" (`<HotkeyRecorder>`) |
| **`recordingMode`** | `<TranscriptionSection>` | "Dictation" (`<select>`) |
| **`inputDevice`** | `<MicrophoneSection>` | "Dictation" (`<select>`) |
| **`language`** | `<TranscriptionSection>` | "Dictation" (`<select>`) |
| **`vadAutoStop` & `vadSilenceMs`** | `<TranscriptionSection>` | "Dictation" (`<input type="checkbox">` & range) |
| **`restoreClipboard`** | `<ClipboardSection>` | "Behavior" (`<input type="checkbox">`) |
| **`keepTextOnClipboard`** | `<ClipboardSection>` | "Behavior" (`<input type="checkbox">`) |
| **`autoApplyTransform`** | `<AIPolishSection>` | "Behavior" (`<input type="checkbox">`) |
| **`polishGateEnabled` / threshold** | `<AIPolishSection>` | "Behavior" (`<input type="checkbox">`) |
| **`restoreEmoji`, `spokenEmoji`, `spokenPunctuation`** | `<TranscriptionSection>` | "Behavior" (`<input type="checkbox">`) |
| **`modelUnloadDelaySecs`** | `<LivePreviewSection>` | "Model Memory" (`<input type="checkbox">`) |
| **`removeFillerWords` & `fillerWords`** | `<TranscriptionSection>` | "Filler Words" (`<input type="checkbox">` & chip list) |
| **`livePreviewEnabled`** | `<LivePreviewSection>` | "Floating Pill" (`<input type="checkbox">`) |

Even in the latest unstaged worktree, **`livePreviewEnabled` is still duplicated**:
1. In `LivePreviewSection` ([`ui/src/settings/DictationPolishSections.tsx`](ui/src/settings/DictationPolishSections.tsx)): *"Show live transcript while recording"*
2. In `SettingsScreen.tsx` ([`ui/src/screens/SettingsScreen.tsx`](ui/src/screens/SettingsScreen.tsx)): *"Show live transcript in the pill while recording"*

---

### 1.2 How to Fix the Duplication & Make It Production-Ready

1. **Delete the Duplicate `livePreviewEnabled` Toggle**:
   Remove the redundant checkbox under `"Floating Pill"` in `SettingsScreen.tsx`. The canonical toggle belongs in `LivePreviewSection`.
2. **Refactor Remaining Sections to Primitives**:
   Replace raw `<h3>`, inline CSS grids, and naked `<select>` / `<input>` elements at the bottom of `SettingsScreen.tsx` with standardized primitives (`Section`, `Row`, `Select`, `Toggle` from `ui/src/settings/primitives.tsx`):
   * **Floating Pill**: Style Picker, Position dropdown, Always show pill toggle.
   * **Appearance**: Theme selector, Reduce Motion toggle.
   * **App Icon**: Light / Blue switcher.
   * **Transcripts**: Directory input and "Open Folder" action.
   * **App Overrides**: Language and Writing Style override tables.
3. **Rebuild Dist Artifacts**:
   Run `cd ui && npm run build` so that `ui/dist/index.html` and compiled assets reflect the cleaned structure.
4. **Automated Regression Test**:
   In `crates/teletype-core/tests/ipc_contract.rs`, add an AST / DOM lint or component test asserting that each key of `Settings` is bound to **at most one** interactive input across the entire rendered component tree.

---

## 2. Prioritized Production Blockers & Defect Catalog

```
[BLOCKER] Rust Compilation Failure (crates/teletype-core/src/transforms/engine.rs)
  │
  ├── [CRITICAL] Transforms Screen Casing Defect (Allows deleting built-in transforms)
  ├── [CRITICAL] Metal/GGML Cleanup Crash on Application Quit (SIGABRT in ggml_abort)
  ├── [HIGH] Cross-Screen State Desynchronization (AutoText & Dictionary Screens)
  ├── [HIGH] Unbounded 33ms Animation Timer Causing Background CPU & Battery Drain
  ├── [MEDIUM] Degraded Input UX for VAD Silence Duration
  └── [MEDIUM] Inconsistent Error States and Stale Keys in ModelsScreen
```

---

### Blocker 1: Workspace Compilation Failure
* **File:** [`crates/teletype-core/src/transforms/engine.rs`](crates/teletype-core/src/transforms/engine.rs)
* **Error:** `E0308: mismatched types` & `E0277: () doesn't implement std::fmt::Display`
* **Root Cause:** In `engine.rs`, `run_eg1` and `run_s1` signatures return `Result<String, ()>`, but the function bodies return `Err(String)`. Meanwhile, the call site at lines 313 and 363 executes `format!("... {detail}")`, expecting a `Display`-able `String`.
* **Fix:** Update both `run_eg1` and `run_s1` signatures to return `Result<String, String>` and return `Err("input too short".to_string())` on short inputs.

---

### Critical 1: Wire Casing Bug in [`ui/src/screens/TransformsScreen.tsx`](ui/src/screens/TransformsScreen.tsx)
* **File:** [`ui/src/screens/TransformsScreen.tsx`](ui/src/screens/TransformsScreen.tsx)
* **Impact:** 
  * Rust’s `TransformDefinition` serializes with `#[serde(rename_all = "camelCase")]`, yielding `builtIn` and `autoApply`.
  * `TransformsScreen.tsx` declares `built_in: boolean; auto_apply: boolean;`.
  * Because `t.built_in` is always `undefined`, `!t.built_in` evaluates to `true` for **all** transforms. As a result, **the "Delete" button is rendered on every built-in transform**, allowing users to permanently delete core transforms like *Polish* or *Professional*.
  * Furthermore, creating a transform sends snake_case keys (`built_in`, `auto_apply`, `sort_order`, `created_at`, `updated_at`), which fail serde contract validation.
* **Fix:** Change `Transform` interface in `TransformsScreen.tsx` to camelCase (`builtIn`, `autoApply`, `sortOrder`, `createdAt`, `updatedAt`) and update all references in the screen. Remove the waiver from `KNOWN_CASING_DEFECTS` in `crates/teletype-core/tests/ipc_contract.rs`.

---

### Critical 2: Application Crash on Exit (SIGABRT)
* **Evidence:** [`Teletype-report.txt`](Teletype-report.txt)
* **Symptom:** Quitting the application via menu bar (`NSApplication terminate:`) triggers `EXC_CRASH (SIGABRT)` in `ggml_metal_rsets_free` / `ggml_abort`.
* **Root Cause:** When `NSApplication` terminates, the C/Metal global device is torn down after the run loop exits while threads or allocations remain referenced.
* **Fix:** Verify `mark_teardown()` before `AppState` drops in the `RunEvent::Exit` hook. Ensure on macOS that shutting down during an active transcription or warm engine state exits with code 0 without generating a crash log in `~/Library/Logs/DiagnosticReports/`.

---

### High 1: Cross-Screen Settings Desynchronization
* **Files:** [`ui/src/screens/AutoTextScreen.tsx`](ui/src/screens/AutoTextScreen.tsx) & [`ui/src/screens/DictionaryScreen.tsx`](ui/src/screens/DictionaryScreen.tsx)
* **Impact:** 
  * `typingAutotextEnabled` is editable in both `AutoTextScreen` and `SettingsScreen -> General`.
  * `AutoTextScreen` fetches settings once on mount and **never subscribes to `settings-changed`**.
  * If a user toggles "Expand AutoText while typing" in Settings and switches back to AutoText, the switch shows the stale value until the entire app is restarted. The same applies to `matchStrictness` in `DictionaryScreen`.
* **Fix:** Add `useTauriEvent<void>("settings-changed", ...)` listeners in `AutoTextScreen.tsx` and `DictionaryScreen.tsx` to re-sync local state.

---

### High 2: Unbounded 33ms Animation Loop Battery Drain
* **File:** [`ui/src/screens/SettingsScreen.tsx`](ui/src/screens/SettingsScreen.tsx)
* **Impact:** `usePillPreviewLevels()` instantiates a `window.setInterval(..., 33)` timer (30 FPS) calculating sine-wave audio levels for the pill preview cards. Because `App.tsx` keeps all screens mounted via `display: none`, **this timer runs continuously forever**, even when the user is on the Home tab or when the app window is hidden in the background, consuming CPU cycles and draining laptop battery.
* **Fix:** Gate the interval behind `document.visibilityState === "visible"` and pass down an `isActive: boolean` prop (or check container visibility) so the timer only ticks when the Settings screen is actively being viewed.

---

### Medium 1: VAD Silence Duration UX Regression
* **File:** [`ui/src/settings/DictationPolishSections.tsx`](ui/src/settings/DictationPolishSections.tsx)
* **Impact:** The legacy screen offered an intuitive slider from 300ms to 2000ms with a 100ms step. In the new `TranscriptionSection`, this was replaced with a plain `TextInput`. If the user clears the input or enters non-numeric text, it silently fails without displaying error feedback.
* **Fix:** Provide a stepped range slider or a formatted number input with explicit min (100ms) / max (3000ms) clamps and visible validation.

---

### Medium 2: Keychain & API Key Error Handling on Models Screen
* **File:** [`ui/src/screens/ModelsScreen.tsx`](ui/src/screens/ModelsScreen.tsx)
* **Impact:** `saveApiKey` and `connectOpenai` capture errors into `testResult` state, but if a connection test subsequently times out or the host is unreachable, the test status banner stays indefinitely and does not distinguish between bad credentials vs. network timeout.

---

## 3. Recommended Production Go-Live Checklist

- [ ] **Step 1: Fix Core Compilation**  
  Update `run_eg1` and `run_s1` in `crates/teletype-core/src/transforms/engine.rs` to return `Result<String, String>` and ensure `cargo test --workspace` passes 100%.
- [ ] **Step 2: Clean up Settings Duplication**  
  Remove `livePreviewEnabled` from Floating Pill and migrate Floating Pill, App Icon, Appearance, and Transcripts into standard `Section` / `Row` primitives.
- [ ] **Step 3: Fix `TransformsScreen.tsx` Casing Contract**  
  Change `built_in` / `auto_apply` to `builtIn` / `autoApply` to safeguard built-in transforms from accidental deletion.
- [ ] **Step 4: Stop Background CPU / Timer Leak**  
  Pause `usePillPreviewLevels()` interval when the Settings tab or the window is hidden.
- [ ] **Step 5: Subscribe to `settings-changed` Across All Screens**  
  Ensure `AutoTextScreen` and `DictionaryScreen` dynamically reflect changes made in Settings.
- [ ] **Step 6: Build & Validate Distribution Bundle**  
  Run `cd ui && npm run build` and verify that the resulting binary bundle launches cleanly, displays zero duplicate controls, and quits without SIGABRT errors.
