# Task: the honesty block (BUG-002 + 003 build-or-remove, BUG-008, BUG-011)

**Status:** closed (verified 2026-10-01)
**Filed:** 2026-09-28 by productivity monitor
**Tickets:** `bugs/BUG-002-spoken-emoji-punctuation-toggles-absent.md`, `bugs/BUG-003-unload-model-timer-noop.md`, `bugs/BUG-008-language-picker-8-vs-99.md`, `bugs/BUG-011-live-preview-hold-mode.md`

## Outcome (all four resolved in code)

- **BUG-002 — fixed.** `spoken_emoji` and `spoken_punctuation` settings exist
  (`commands.rs` Settings, both `default_true`) and both have live engine
  paths: `crates/teletype-core/src/transforms/spoken_emoji.rs` (spoken emoji
  phrases → glyphs, ported from EW's EmojiFormatterStep) and
  `crates/teletype-core/src/autotext/system.rs` (spoken-punctuation AutoText
  entries), both wired through `pipeline.rs` and `dictation.rs`.
- **BUG-003 — fixed.** The idle-unload timer is fully wired:
  `SpeechModelManager::end_dictation(delay)` arms the deadline on session end
  (`dictation.rs`, both cancel and normal-completion paths), a 5 s background
  tick thread in `teletype-desktop/src/lib.rs` calls `tick()` on both the
  `speech` and `parakeet` managers, and `manager.rs` fires `unload()` when the
  deadline passes with no active dictation. 2026-10-01 hardening: the
  launch-time warm-up path now disarms a pending deadline
  (`end_dictation(None)`) before loading, so a warm-up cannot be undone
  seconds later by the tick thread; `manager.rs` gained an end-to-end test
  (`end_dictation_arms_unload_that_tick_fires_on_a_real_provider`) covering
  the arm → tick → unload sequence on a real `SpeechProvider` plus the
  Never-policy case.
- **BUG-008 — fixed (2026-09-28).** The language picker exposes the full
  whisper.cpp table via `speech_languages_static`; the UI label derives from
  the list length.
- **BUG-011 — fixed (2026-09-28).** The interim loop runs in Hold mode too;
  `live_preview_enabled` (default on) is consumed in `dictation.rs`.

## Verification (2026-10-01)

- `cargo test --workspace` green (includes the new end-to-end unload test).
- `cd ui && npx tsc --noEmit` clean.
- Brain/README/usp.md re-grepped: no claim without an engine path.
