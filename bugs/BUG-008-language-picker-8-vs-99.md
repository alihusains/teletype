# BUG-008 — Language picker offers 8 languages behind a "99 languages" label; cosmetic on Windows

**Severity:** P1 (user-facing overstatement + dead control on Windows)
**Area:** desktop / commands.rs + ui
**Status:** open
**Found:** 2026-09-28 QA audit (5-agent sweep), verified by reading `commands.rs:1516-1542` and grepping ui

---

## Description

`list_speech_languages()` (`commands.rs:1516-1542`) returns a static 8-entry
table (en, zh, de, es, ru, ko, fr, pt) with a comment claiming the "static
subset" is "platform-agnostic". But the UI label says **"Auto-detect (99
languages)"** (`ui/src/screens/SettingsScreen.tsx:393-399`) and the dropdown
lists only what the backend returns.

A Spanish/Italian/Dutch/Hindi speaker who wants their language — all present in
the 99-language Whisper model — **cannot select it**; the dropdown silently
truncates. The label overstates by ~12×.

### Second half: cosmetic on Windows

On Windows the picker is 100% cosmetic because the only engine that honors
`language` (Whisper) is a stub there:

- `crates/teletype-speech/src/whisper/stub.rs:38-40` — `load` → "not built for
  this platform", `transcribe` → `Err(NoModel)`.
- `crates/teletype-speech/Cargo.toml:16-20` — whisper-rs is macOS-only.
- `crates/teletype-speech/src/parakeet.rs:166` — Parakeet ignores the language
  arg by design (`_language`).

So on Windows (equal priority per decision D005) the user picks a language and
**nothing changes**.

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-desktop/src/commands.rs` | 1516-1542 | `list_speech_languages` + `speech_languages_static` (8-entry table) |
| `ui/src/screens/SettingsScreen.tsx` | 393-399 | the "99 languages" label |
| `crates/teletype-speech/src/whisper/stub.rs` | 38-40 | Whisper stub on Windows |
| `crates/teletype-speech/Cargo.toml` | 16-20 | whisper-rs macOS-only |
| `crates/teletype-speech/src/parakeet.rs` | 166 | Parakeet ignores language |

## Reproduction

1. Settings → Transcription → Language.
2. Note the label says "99 languages" but only 8 options appear.
3. Try to select Italian or Hindi — not available.
4. (Windows) select any language — transcription language does not change.

## Unit test cases (must pass after fix)

```rust
// 1. The advertised language count must match what the backend actually offers.
#[test]
fn language_count_label_matches_list_length() {
    let langs = list_speech_languages();
    // The UI label "Auto-detect (N languages)" is rendered from this count.
    assert_eq!(LANG_LABEL_COUNT, langs.len(),
        "label overstates the picker — user cannot select unlisted languages");
}

// 2. If the full Whisper table is exposed, it must include the long-tail languages.
#[test]
fn language_list_matches_whisper_model_support() {
    let langs = list_speech_languages();
    for expected in ["it", "hi", "nl", "ja", "pl"] {
        assert!(langs.iter().any(|l| l.code == expected),
            "language {expected} is supported by the Whisper model but not offered");
    }
}

// 3. On a platform where Whisper is a stub, the engine must report itself as
//    language-incapable so the UI can hide/label the picker honestly.
#[test]
fn stub_engine_reports_language_unavailable() {
    let engine = WhisperEngine::for_current_platform();
    if is_windows_stub() {
        assert!(!engine.supports_language_selection(),
            "picker promises per-language transcription on a platform where Whisper is a stub");
    }
}
```

## Acceptance criteria

- [ ] The language list exposes the full Whisper table (it is already in the
      model), **or** the UI label is changed to match the 8 actually offered.
- [ ] On Windows the picker is hidden or clearly labeled "not available on this
      platform".
- [ ] No user-facing overstatement: the advertised count equals the options
      shown.
- [ ] `cargo test -p teletype-desktop` and the UI tests pass.

## How to test (manual / smoke)

1. **Smoke (1 min):**
   - Settings → Transcription → Language.
   - **Pass:** the number of language options equals what the label claims.
   - **Fail:** label says 99, dropdown shows 8.

2. **Long-tail language (1 min):**
   - Try to select Italian / Hindi / Dutch.
   - **Pass:** available and selectable (full table exposed).
   - **Fail:** silently absent.

3. **Regression (1 min):**
   - Select "auto" and transcribe.
   - **Pass:** "auto" still passes through to Whisper.
   - **Windows build:** the picker does not promise languages it cannot deliver
     (hidden or labeled unavailable).

## Fix direction

Preferred: walk the real whisper.cpp language table (the prior implementation
did, macOS-only) and expose all 99 — the data is already in the model. Change
`speech_languages_static` to the full table and derive the UI label count from
the list length so the two can never drift.

Alternative (honest but regressive): keep the 8-entry table and change the UI
label to "8 languages".

Either way, gate the per-language options on the selected engine's capability
(`supports_language_selection`) so Windows does not show a dead picker.

## Related

- Decision D005 — Windows equal priority
- Brain note 2026-09-25 — language-picker decision
