# BUG-002 — Spoken-emoji and spoken-punctuation toggles are advertised but do not exist

**Severity:** P0 (advertised feature, complete no-op)
**Area:** settings / transforms / EW-parity
**Status:** open
**Found:** 2026-09-28 QA audit, verified by grep across `crates/` and `ui/src/`

---

## Description

The project brain's 2026-09-25 decision log lists **"(3) spoken emoji + spoken
punctuation toggles"** as an adopted EW feature. **Nothing was ported.**

- `grep -rn "spoken_emoji\|spoken_punctuation" crates/` → **0 hits**
- `grep -rn "spokenEmoji\|spokenPunctuation" ui/src/` → **0 hits**
- No setting field, no UI control, no engine path.

EW has both, wired end-to-end:
- `ew/.../Views/Settings/SpeechEngineSettingsView.swift:357-383` —
  `emojiFormatterEnabled` ("Convert spoken emoji") and
  `spokenPunctuationEnabled` ("Convert spoken punctuation") toggles.
- `ew/.../PipelineSettingsSync.swift:167-168, 309-314` — wired to the pipeline.
- `ew/.../PostProcessing/EmojiFormatterStep.swift` — say "thumbs up emoji" → glyph.

### What Teletype actually has (and it is NOT the same feature)

- `restore_emoji: bool` (`commands.rs:241`) — a **different** feature
  (words→glyph *after* transform), with **no UI toggle at all**. It is
  ratcheted as "P3.2 shipped with no toggle" in
  `crates/teletype-core/tests/ipc_contract.rs:589`.
- Spoken punctuation: Teletype's mechanism is System AutoText
  (`autotext/system.rs:36+` — "comma" → `,`), which is **always on, not
  toggleable**, and covers a different phrase set than EW's `punct` table
  (`ew/.../InverseTextNormalizer.swift:2176-2181`).

### Business impact

A user reading the roadmap or the brain sees "spoken emoji + spoken punctuation
toggles — adopted from EW." They open Settings, find no such toggles. If this
is in the public README or a demo, it is a credibility hit against Whisperflow,
which ships both.

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-desktop/src/commands.rs` | 241 | `restore_emoji` (different feature, no UI) |
| `crates/teletype-core/src/autotext/system.rs` | 36+ | System AutoText (always-on, not toggleable) |
| `crates/teletype-core/tests/ipc_contract.rs` | 589 | ratchets `restore_emoji` as no-UI |
| `ew/EnviousWispr/Sources/EnviousWisprAppKit/Views/Settings/SpeechEngineSettingsView.swift` | 357-383 | EW reference: both toggles |
| `ew/EnviousWispr/Sources/EnviousWisprPostProcessing/EmojiFormatterStep.swift` | — | EW reference: emoji formatter |
| `ew/EnviousWispr/Sources/EnviousWisprPostProcessing/InverseTextNormalizer.swift` | 2176-2181 | EW reference: spoken punctuation table |

## Reproduction

1. Open Settings → Transcription tab.
2. Look for "Convert spoken emoji" or "Convert spoken punctuation" toggles.
3. **They do not exist.** The only emoji-related setting is `restore_emoji`,
   which has no UI control.

## Unit test cases (must pass after fix)

```rust
// 1. The setting exists and defaults to the EW default (on).
#[test]
fn spoken_emoji_setting_defaults_on() {
    let s = Settings::default();
    assert!(s.spoken_emoji);
}

#[test]
fn spoken_punctuation_setting_defaults_on() {
    let s = Settings::default();
    assert!(s.spoken_punctuation);
}

// 2. Toggling spoken-emoji off means "thumbs up emoji" in the input
//    stays as the words, not a glyph.
#[test]
fn spoken_emoji_off_leaves_words_alone() {
    let out = transform("thanks for the thumbs up emoji", /* emoji: */ false);
    assert_eq!(out, "thanks for the thumbs up emoji");
}

// 3. Toggling spoken-emoji on converts the spoken form to a glyph.
#[test]
fn spoken_emoji_on_converts_to_glyph() {
    let out = transform("thanks for the thumbs up emoji", /* emoji: */ true);
    assert!(out.contains("👍"), "expected a thumbs-up glyph, got: {out}");
}

// 4. Toggling spoken-punctuation off means "comma" stays as the word.
#[test]
fn spoken_punctuation_off_leaves_word() {
    let out = transform("put a comma here", /* punct: */ false);
    assert!(out.contains("comma"), "expected the word 'comma', got: {out}");
}

// 5. Toggling spoken-punctuation on converts "comma" to ",".
#[test]
fn spoken_punctuation_on_converts_to_symbol() {
    let out = transform("put a comma here", /* punct: */ true);
    assert!(out.contains(','), "expected a comma, got: {out}");
}

// 6. The setting is persisted and rehydrated across a restart.
#[test]
fn spoken_settings_survive_restart() {
    let mut s = Settings::default();
    s.spoken_emoji = false;
    s.spoken_punctuation = false;
    let json = serde_json::to_string(&s).unwrap();
    let s2: Settings = serde_json::from_str(&json).unwrap();
    assert!(!s2.spoken_emoji);
    assert!(!s2.spoken_punctuation);
}
```

## Acceptance criteria

- [ ] `Settings` has `spoken_emoji: bool` (default `true`) and
      `spoken_punctuation: bool` (default `true`).
- [ ] UI has two toggles in the Transcription tab, matching EW's labels.
- [ ] The emoji formatter runs in the pipeline when `spoken_emoji` is true and
      is skipped when false.
- [ ] The spoken-punctuation pass (System AutoText's symbol entries) is gated
      on `spoken_punctuation`; when false, "comma"/"period"/"star" stay as words.
- [ ] Both settings are persisted in `settings.json` and rehydrated at startup.
- [ ] `ipc_contract.rs` is updated to include the two new fields.
- [ ] `cargo test -p teletype-core` passes.

## How to test (manual / smoke)

1. **Smoke (1 min):**
   - Settings → Transcription → verify both toggles exist and default ON.
   - Toggle both OFF.
   - Dictate: "thanks for the thumbs up emoji, put a comma here."
   - **Pass (off):** text contains the words "thumbs up emoji" and "comma".
   - Toggle both ON, dictate the same sentence.
   - **Pass (on):** text contains "👍" and ",".

2. **Persistence (2 min):**
   - Set both OFF, restart the app.
   - **Pass:** both are still OFF.

3. **EW parity check:**
   - Compare the toggle labels and default states against
     `SpeechEngineSettingsView.swift:357-383`.
   - **Pass:** labels and defaults match.

## Fix direction

1. Add `spoken_emoji` and `spoken_punctuation` to `Settings` (serde camelCase:
   `spokenEmoji`, `spokenPunctuation`), default `true`.
2. Add a `SpokenEmojiFormatter` pass in `transforms/` (port EW's
   `EmojiFormatterStep` — it is a phrase→glyph table, ~50 entries).
3. Gate the System AutoText symbol entries on `spoken_punctuation` in
   `autotext/system.rs` (the entries, not the whole engine — "comma" should
   still work as an AutoText trigger when the toggle is off, just not expand
   to a symbol).
4. Add the two toggles to `SettingsScreen.tsx` Transcription section.
5. Wire both through the pipeline in `pipeline.rs`.

## Related

- BUG-003 (unload timer — the other advertised-but-absent feature)
- `restore_emoji` (`commands.rs:241`) is a different feature and should keep
  its own (currently missing) UI toggle.
