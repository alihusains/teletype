# BUG-003 — "Unload model after" timer is a complete no-op

**Severity:** P0 (advertised feature, zero implementation)
**Area:** inference / speech / lifecycle
**Status:** open
**Found:** 2026-09-28 QA audit, verified by grep (0 hits for `unload_after`,
`model_unload`, `unload_timer` in `crates/` and `ui/src/`)

---

## Description

The project brain's 2026-09-25 decision log lists **"(5) unload-model-after
timer"** as an adopted EW feature. **Nothing was implemented.**

- No `unload_after` / `model_unload_policy` setting in `commands.rs` or `ui/src/`.
- `ModelManager` (`crates/teletype-inference/src/manager.rs:100-106`) *does*
  implement `unload()` (drops the provider `Box`), but **`ModelManager` has
  zero callers outside its own crate** — no import in `teletype-desktop`.
- The speech providers implement `unload()` faithfully
  (`parakeet.rs:152-159` `parakeet_free`, `whisper/macos.rs:54-57` drops
  `context`/`state`), but **nothing in `teletype-desktop` ever calls it**.
- The speech model is loaded once (`dictation.rs:689-734`) and stays in RAM
  for the life of the app.

EW ships a 7-option policy:
- `ew/.../AppSettings.swift:66-73` — `ModelUnloadPolicy`:
  never / immediately / 2m / 5m / 10m / 15m / 60m.
- `ew/.../SpeechEngineSettingsView.swift:389-410` — picker UI.
- `ew/.../WhisperKitEngineAdapter.swift:1086+` — `applyUnloadPolicy`,
  session-keyed delayed `backend.unload()`.
- `ew/.../ParakeetEngineAdapter.swift:937` — Parakeet variant.

### Business impact

A user who picks "Unload model after 5 minutes" (per the roadmap) gets no such
control. The model never frees RAM. On a laptop with 16 GB, a 2.7 GB speech
model plus a 5.4 GB LLM model leaves little headroom for the user's actual
work. EW users get the control; Teletype users don't.

### Structural cause

`ModelManager` was built (the lifecycle state machine
Unavailable→…→Unloading, `unload()` at `:100`) and exported (`lib.rs:22`), but
never connected. The app manages speech providers directly via
`Mutex<Box<dyn SpeechProvider>>` (`lib.rs:334`, `dictation.rs:689-734`). This
is the structural reason the timer doesn't exist: the lifecycle layer was
built but never wired.

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-inference/src/manager.rs` | 100-106 | `unload()` — implemented, never called |
| `crates/teletype-inference/src/manager.rs` | — | `ModelState` — exported, never produced |
| `crates/teletype-speech/src/parakeet.rs` | 152-159 | `parakeet_free` — faithful, never called |
| `crates/teletype-speech/src/whisper/macos.rs` | 54-57 | drop context/state — never called |
| `crates/teletype-desktop/src/dictation.rs` | 689-734 | model loaded once, never unloaded |
| `crates/teletype-desktop/src/lib.rs` | 334 | `Mutex<Box<dyn SpeechProvider>>` — direct, no manager |
| `ew/EnviousWispr/Sources/EnviousWisprCore/AppSettings.swift` | 66-73 | EW: 7-option policy |
| `ew/EnviousWispr/Sources/EnviousWisprAppKit/Views/Settings/SpeechEngineSettingsView.swift` | 389-410 | EW: picker UI |
| `ew/EnviousWispr/Sources/EnviousWisprAudio/WhisperKitEngineAdapter.swift` | 1086+ | EW: applyUnloadPolicy |

## Reproduction

1. Select a speech model (Parakeet or Whisper).
2. Open Activity Monitor → find the Teletype process.
3. Note the RAM usage (includes the model).
4. Wait 10 minutes (or any duration).
5. **The model is still in RAM.** There is no setting to change this.

## Unit test cases (must pass after fix)

```rust
// 1. The setting exists with the EW 7-option enum.
#[test]
fn unload_policy_defaults_to_never() {
    let s = Settings::default();
    assert_eq!(s.model_unload_policy, ModelUnloadPolicy::Never);
}

#[test]
fn unload_policy_variants_serialize() {
    for p in [
        ModelUnloadPolicy::Never,
        ModelUnloadPolicy::Immediately,
        ModelUnloadPolicy::Minutes(2),
        ModelUnloadPolicy::Minutes(5),
        ModelUnloadPolicy::Minutes(10),
        ModelUnloadPolicy::Minutes(15),
        ModelUnloadPolicy::Minutes(60),
    ] {
        let json = serde_json::to_string(&p).unwrap();
        let p2: ModelUnloadPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(p, p2);
    }
}

// 2. Unloading actually frees the model (provider reports unloaded).
#[test]
fn unload_frees_the_model() {
    let mut provider = MockSpeechProvider::loaded();
    assert!(provider.is_loaded());
    provider.unload();
    assert!(!provider.is_loaded());
}

// 3. Unloading does NOT fire while a dictation is in flight.
#[test]
fn unload_does_not_fire_during_active_dictation() {
    let mut manager = ModelManager::new();
    manager.load();
    manager.begin_dictation();
    manager.arm_unload_after(Duration::from_millis(0)); // immediate
    manager.tick(); // should be a no-op while dictation is active
    assert!(manager.is_loaded(), "model must not unload mid-dictation");
}

// 4. Unload fires after the policy duration once dictation ends.
#[test]
fn unload_fires_after_policy_duration() {
    let mut manager = ModelManager::new();
    manager.load();
    manager.arm_unload_after(Duration::from_millis(1));
    manager.end_dictation();
    std::thread::sleep(Duration::from_millis(5));
    manager.tick();
    assert!(!manager.is_loaded(), "model must unload after the policy duration");
}

// 5. The next dictation after an unload cold-starts (re-loads the model).
#[test]
fn next_dictation_after_unload_reloads() {
    let mut manager = ModelManager::new();
    manager.load();
    manager.unload();
    manager.begin_dictation();
    assert!(manager.is_loaded(), "model must be re-loaded on next dictation");
}
```

## Acceptance criteria

- [ ] `Settings` has `model_unload_policy: ModelUnloadPolicy` (default
      `Never`), serialized as camelCase `modelUnloadPolicy`.
- [ ] `ModelUnloadPolicy` enum: `Never`, `Immediately`, `Minutes(2)`,
      `Minutes(5)`, `Minutes(10)`, `Minutes(15)`, `Minutes(60)` — matching EW.
- [ ] UI has a picker in the Transcription tab with the 7 options.
- [ ] `ModelManager` is wired into `teletype-desktop` and owns the
      `SpeechProvider` lifecycle (replaces the direct
      `Mutex<Box<dyn SpeechProvider>>`).
- [ ] Unload is **never** armed while a dictation session is active.
- [ ] The next dictation after an unload re-loads the model (cold start is
      acceptable; document the expected latency).
- [ ] `cargo test -p teletype-inference -p teletype-desktop` passes.

## How to test (manual / smoke)

1. **Smoke (2 min):**
   - Settings → Transcription → verify the "Unload model after" picker exists
     with 7 options, defaulting to "Never".
   - Select "2 minutes".
   - Dictate once (model loads, RAM rises).
   - Wait 2 minutes without dictating.
   - **Pass:** RAM drops (model unloaded).
   - Dictate again.
   - **Pass:** model re-loads, dictation works (cold start, slower).

2. **Mid-dictation safety (3 min):**
   - Set policy to "Immediately".
   - Start dictating (hold the hotkey, speak).
   - **Pass:** the model does NOT unload mid-dictation; the take completes.

3. **RAM check (2 min):**
   - Open Activity Monitor, filter for Teletype.
   - Load a model, note RAM.
   - Set policy to "Immediately", end a dictation.
   - **Pass:** RAM drops by roughly the model size within a few seconds.

## Fix direction

1. Add `ModelUnloadPolicy` enum + `model_unload_policy` to `Settings`.
2. Wire `ModelManager` into `teletype-desktop` (replace the direct
   `Mutex<Box<dyn SpeechProvider>>` in `lib.rs:334`).
3. In `ModelManager`, after `end_dictation()`, arm a delayed `unload()` based
   on the policy. Cancel the delay on `begin_dictation()`.
4. Add the picker to `SettingsScreen.tsx`.
5. Emit a `model-unloaded` event so the UI can show "model unloaded" state.

## Related

- BUG-002 (the other advertised-but-absent feature)
- F6 in the inference audit (orphan llama-server) — the LLM-side lifecycle has
  its own issues; this ticket is the speech-side lifecycle.
