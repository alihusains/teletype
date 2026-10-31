//! P9.2 — migration test: v0.1.0 / v0.2.0 / v0.2.6 fixtures must still load.
//!
//! User files are not in the repo, so the fixtures under
//! `tests/fixtures/migration/` were DERIVED FROM THE OLD CODE: each one
//! replicates exactly what the serde structs at that tag would have written
//! to disk (same top-level shape, same field names, including fields that
//! no longer exist today).
//!
//! Fixture provenance (tag -> old source path that defined the on-disk shape):
//!
//! - `settings-v0.1.0.json`  <- `git show v0.1.0:crates/teletype-desktop/src/commands.rs`
//!   (`pub struct Settings` at that tag; 31 fields, no `keepTextOnClipboard`)
//! - `settings-v0.2.0.json`  <- `git show v0.2.0:crates/teletype-desktop/src/commands.rs`
//!   (adds `keepTextOnClipboard`, `spokenEmoji`, `spokenPunctuation`,
//!   `modelUnloadDelaySecs`, `livePreviewEnabled`)
//! - `settings-v0.2.6.json`  <- `git show v0.2.6:crates/teletype-desktop/src/commands.rs`
//!   (adds `autotextTiming`, `playRecordingSounds`, `recordingSound`,
//!   `cancelHotkey`, `smartInsertion`, `warmEnginePolicy`, `matchStrictness`,
//!   `dictionaryEnabled`, `polishRules`, `listStyle`,
//!   `polishCustomInstructions`, `quickAddHotkey`)
//! - `dictionary-v0.1.0.json` <- `git show v0.1.0:crates/teletype-core/src/dictionary.rs`
//!   (word fields: id, word, pronunciation, createdAt, fuzzy)
//! - `dictionary-v0.2.0.json` <- `git show v0.2.0:crates/teletype-core/src/dictionary.rs`
//!   (adds learnedFrom, learnedAt)
//! - `dictionary-v0.2.6.json` <- `git show v0.2.6:crates/teletype-core/src/dictionary.rs`
//!   (adds aliases, usageCount)
//! - `history-v0.1.0.json`    <- `git show v0.1.0:crates/teletype-core/src/history.rs`
//!   (entry fields: id, createdAt, text, context; no `durationMs`)
//! - `history-v0.2.0.json`    <- `git show v0.2.0:crates/teletype-core/src/history.rs`
//!   (adds `durationMs`)
//! - `history-v0.2.6.json`    <- `git show v0.2.6:crates/teletype-core/src/history.rs`
//!   (same shape as v0.2.0)
//!
//! On-disk file names used by the app today (see `crates/teletype-desktop/src/lib.rs`):
//! `settings.json`, `dictionary.json`, `dictation.json`.
//!
//! ## Seam note (settings)
//!
//! The settings document type `commands::Settings` lives in a private
//! module; the integration test cannot name it. The real loader the app
//! uses is `JsonStore::<Settings>::new(dir, "settings.json").load(...)`
//! (see `AppState::new` in `lib.rs`). `lib.rs` re-exports the type at the
//! crate root (`pub use commands::Settings`, same test-seam pattern as
//! `decode_audio_file_public`), so this test drives the real loader
//! directly: the fixture file is placed at `<temp>/settings.json` and read
//! back through a `JsonStore<Settings>` built exactly the way
//! `AppState::new` builds it.
//!
//! ## The removed-field question (plan P2.2, `command_mode_active`)
//!
//! `git log -S command_mode_active` finds no commit that ever added or
//! removed such a field, and no occurrence remains in the current tree or
//! in any of the three tags. Whatever "serde-compatible migration" the plan
//! refers to, the observable contract is the same: `Settings` is
//! `#[serde(rename_all = "camelCase", default)]` with no
//! `deny_unknown_fields`, so unknown/removed fields are ignored and missing
//! fields take defaults. The v0.2.6 fixture therefore carries a bogus
//! `commandModeActive` field as the removed-field stand-in, and the test
//! asserts the load still succeeds.
//!
//! All three artifacts existed in all three tags, so every artifact x
//! version combination is covered (no fabricated gaps).

use std::path::{Path, PathBuf};

use teletype_core::dictionary::Dictionary;
use teletype_core::history::open_history;
use teletype_core::storage::JsonStore;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/migration");

/// Copies `fixture_name` into a fresh temp dir under the app's real file
/// name and returns the temp dir (held open for the test's lifetime).
fn load_fixture(fixture_name: &str, file_name: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let src = Path::new(FIXTURES).join(fixture_name);
    let dst = tmp.path().join(file_name);
    std::fs::copy(&src, &dst).expect("copy fixture into temp dir");
    (tmp, dst)
}

// ---------------------------------------------------------------------------
// Settings — loaded through the same JsonStore the app builds in
// `AppState::new` (lib.rs:149). The store's path is fixed at construction,
// so the fixture is placed at the exact path the store was built for.
// ---------------------------------------------------------------------------

/// Loads the settings fixture through the crate-root re-export
/// `teletype_desktop_lib::Settings`, with a `JsonStore` built exactly the
/// way `AppState::new` builds its `settings_store`.
fn load_settings_fixture(
    fixture_name: &str,
) -> (tempfile::TempDir, teletype_desktop_lib::Settings) {
    let (tmp, _path) = load_fixture(fixture_name, "settings.json");
    let dir = tmp.path().to_path_buf();
    // Build the store exactly as AppState::new does.
    let store: JsonStore<teletype_desktop_lib::Settings> = JsonStore::new(&dir, "settings.json");
    let settings = store.load(Default::default());
    (tmp, settings)
}

// ---------------------------------------------------------------------------
// Settings tests
// ---------------------------------------------------------------------------

#[test]
fn settings_v0_1_0_loads() {
    let (_tmp, s) = load_settings_fixture("settings-v0.1.0.json");

    // User values survive.
    assert_eq!(s.hotkey, "cmd+shift+space");
    assert_eq!(s.language, "de");
    assert!(!s.restore_clipboard);
    // `blue` was a v0.1.0 icon choice that no longer exists; the removed
    // variant lands on `AppIcon::Unknown`, which reports the default `white`.
    assert_eq!(s.app_icon.as_id(), "white");
    assert!(s.filler_words.iter().any(|w| w == "äh"));
    assert_eq!(
        s.app_language_overrides
            .get("com.google.gmail")
            .map(String::as_str),
        Some("en")
    );
    // Fields added after v0.1.0 take their serde defaults.
    assert_eq!(s.theme.as_id(), "system");
    assert!(
        s.keep_text_on_clipboard,
        "added in v0.2.0, defaults to true"
    );
    assert!(
        !s.tap_to_paste_clipboard,
        "old settings files must load with quick-tap paste OFF"
    );
    assert_eq!(
        s.autotext_timing.as_id(),
        "delimiter",
        "added in v0.2.6, defaults to delimiter"
    );
}

#[test]
fn settings_v0_2_0_loads() {
    let (_tmp, s) = load_settings_fixture("settings-v0.2.0.json");

    assert_eq!(s.hotkey, "fn");
    assert_eq!(s.selected_llm_model, "qwen2.5-3b");
    assert!(!s.spoken_emoji);
    assert!(s.spoken_punctuation);
    assert_eq!(s.model_unload_delay_secs, 30);
    assert_eq!(s.enabled_packs, vec!["tech".to_string()]);
    assert_eq!(s.theme.as_id(), "dark");
    // Fields added after v0.2.0 take defaults.
    assert!(s.play_recording_sounds, "added in v0.2.6, defaults to true");
    assert_eq!(
        s.autotext_timing.as_id(),
        "delimiter",
        "added in v0.2.6, defaults to delimiter"
    );
}

#[test]
fn settings_v0_2_6_loads_and_ignores_the_removed_field() {
    let (_tmp, s) = load_settings_fixture("settings-v0.2.6.json");

    assert_eq!(s.hotkey, "option+d");
    assert_eq!(s.match_strictness.as_id(), "strict");
    assert!(!s.smart_insertion);
    assert_eq!(s.list_style.as_id(), "numbered");
    assert_eq!(s.polish_custom_instructions.len(), 1);
    // The removed-field stand-in (bogus "commandModeActive" in the fixture)
    // must not fail the load: unknown fields are simply ignored.
    assert_eq!(s.quick_add_hotkey, "cmd+shift+l");
}

#[test]
fn settings_round_trip_v0_1_0() {
    let (tmp, s) = load_settings_fixture("settings-v0.1.0.json");
    let dir = tmp.path().to_path_buf();
    let store: JsonStore<teletype_desktop_lib::Settings> = JsonStore::new(&dir, "settings.json");
    store.save(&s).expect("re-save with today's writer");

    let s2 = store.load(Default::default());
    assert_eq!(s2.hotkey, "cmd+shift+space");
    assert_eq!(s2.language, "de");
    assert!(!s2.restore_clipboard);
    // The removed `blue` icon re-saves as the default `white`.
    assert_eq!(s2.app_icon.as_id(), "white");
    assert_eq!(
        s2.app_language_overrides
            .get("com.google.gmail")
            .map(String::as_str),
        Some("en")
    );
    // The re-saved file is the current shape: no leftover old-only fields.
    let raw = std::fs::read_to_string(dir.join("settings.json")).expect("read re-saved settings");
    assert!(!raw.contains("commandModeActive"));
}

#[test]
fn settings_round_trip_v0_2_0() {
    let (tmp, s) = load_settings_fixture("settings-v0.2.0.json");
    let dir = tmp.path().to_path_buf();
    let store: JsonStore<teletype_desktop_lib::Settings> = JsonStore::new(&dir, "settings.json");
    store.save(&s).expect("re-save with today's writer");

    let s2 = store.load(Default::default());
    assert_eq!(s2.hotkey, "fn");
    assert_eq!(s2.selected_llm_model, "qwen2.5-3b");
    assert!(!s2.spoken_emoji);
    assert_eq!(s2.model_unload_delay_secs, 30);
    assert_eq!(s2.enabled_packs, vec!["tech".to_string()]);
    assert_eq!(s2.theme.as_id(), "dark");
}

#[test]
fn settings_round_trip_v0_2_6() {
    let (tmp, s) = load_settings_fixture("settings-v0.2.6.json");
    let dir = tmp.path().to_path_buf();
    let store: JsonStore<teletype_desktop_lib::Settings> = JsonStore::new(&dir, "settings.json");
    store.save(&s).expect("re-save with today's writer");

    let s2 = store.load(Default::default());
    assert_eq!(s2.hotkey, "option+d");
    assert_eq!(s2.match_strictness.as_id(), "strict");
    assert!(!s2.smart_insertion);
    assert_eq!(s2.list_style.as_id(), "numbered");
    assert_eq!(s2.polish_custom_instructions.len(), 1);
    // The bogus removed field is gone after the re-save.
    let raw = std::fs::read_to_string(dir.join("settings.json")).expect("read re-saved settings");
    assert!(!raw.contains("commandModeActive"));
}

// ---------------------------------------------------------------------------
// Dictionary — loaded through the same JsonStore the app builds in
// `AppState::new` (lib.rs:170).
// ---------------------------------------------------------------------------

#[test]
fn dictionary_v0_1_0_loads() {
    let (_tmp, path) = load_fixture("dictionary-v0.1.0.json", "dictionary.json");
    let store = JsonStore::<Dictionary>::new(path.parent().expect("parent"), "dictionary.json");
    let d = store.load(Dictionary::default());

    assert_eq!(d.words.len(), 3);
    let teletype = d
        .words
        .iter()
        .find(|w| w.word == "Teletype")
        .expect("Teletype");
    assert_eq!(teletype.pronunciation, "tel-uh-type");
    // v0.1.0 had no learnedFrom/learnedAt/aliases/usageCount fields: they
    // must default (aliases empty, usage 0) rather than break the load.
    assert!(teletype.learned_from.is_none());
    assert!(teletype.aliases.is_empty());
    assert_eq!(teletype.usage_count, 0);
    assert_eq!(d.builtin_version, 1);
}

#[test]
fn dictionary_v0_2_0_loads() {
    let (_tmp, path) = load_fixture("dictionary-v0.2.0.json", "dictionary.json");
    let store = JsonStore::<Dictionary>::new(path.parent().expect("parent"), "dictionary.json");
    let d = store.load(Dictionary::default());

    assert_eq!(d.words.len(), 3);
    let markets = d
        .words
        .iter()
        .find(|w| w.word == "Markets")
        .expect("Markets");
    assert_eq!(markets.learned_from.as_deref(), Some("margets"));
    assert!(markets.learned_at.is_some());
    // learnedAt: 0 serializes as null in the fixture (Option<u64>); the
    // important contract is that the load succeeds and learnedFrom survives.
    assert_eq!(d.builtin_version, 2);
}

#[test]
fn dictionary_v0_2_6_loads() {
    let (_tmp, path) = load_fixture("dictionary-v0.2.6.json", "dictionary.json");
    let store = JsonStore::<Dictionary>::new(path.parent().expect("parent"), "dictionary.json");
    let d = store.load(Dictionary::default());

    assert_eq!(d.words.len(), 3);
    let teletype = d
        .words
        .iter()
        .find(|w| w.word == "Teletype")
        .expect("Teletype");
    assert_eq!(teletype.aliases, vec!["tel-uh-type".to_string()]);
    assert_eq!(teletype.usage_count, 7);
    assert_eq!(d.builtin_version, 2);
}

#[test]
fn dictionary_round_trip_v0_1_0() {
    let (_tmp, path) = load_fixture("dictionary-v0.1.0.json", "dictionary.json");
    let dir = path.parent().expect("parent").to_path_buf();
    let store = JsonStore::<Dictionary>::new(&dir, "dictionary.json");
    let mut d = store.load(Dictionary::default());
    d.seed_builtins();
    store.save(&d).expect("re-save with today's writer");

    let d2 = store.load(Dictionary::default());
    // User words survive the round trip with their values intact.
    let teletype = d2
        .words
        .iter()
        .find(|w| w.word == "Teletype")
        .expect("Teletype");
    assert_eq!(teletype.pronunciation, "tel-uh-type");
    assert!(teletype.fuzzy);
    // The seed merge ran (builtinVersion 1 -> 2) and user words were kept.
    assert_eq!(d2.builtin_version, 2);
    assert!(d2.words.iter().any(|w| w.word == "Kubernetes"));
}

#[test]
fn dictionary_round_trip_v0_2_0() {
    let (_tmp, path) = load_fixture("dictionary-v0.2.0.json", "dictionary.json");
    let dir = path.parent().expect("parent").to_path_buf();
    let store = JsonStore::<Dictionary>::new(&dir, "dictionary.json");
    let d = store.load(Dictionary::default());
    store.save(&d).expect("re-save with today's writer");

    let d2 = store.load(Dictionary::default());
    let markets = d2
        .words
        .iter()
        .find(|w| w.word == "Markets")
        .expect("Markets");
    assert_eq!(markets.learned_from.as_deref(), Some("margets"));
    assert_eq!(d2.builtin_version, 2);
    assert!(d2.words.iter().any(|w| w.word == "Teletype"));
}

#[test]
fn dictionary_round_trip_v0_2_6() {
    let (_tmp, path) = load_fixture("dictionary-v0.2.6.json", "dictionary.json");
    let dir = path.parent().expect("parent").to_path_buf();
    let store = JsonStore::<Dictionary>::new(&dir, "dictionary.json");
    let d = store.load(Dictionary::default());
    store.save(&d).expect("re-save with today's writer");

    let d2 = store.load(Dictionary::default());
    let teletype = d2
        .words
        .iter()
        .find(|w| w.word == "Teletype")
        .expect("Teletype");
    assert_eq!(teletype.aliases, vec!["tel-uh-type".to_string()]);
    assert_eq!(teletype.usage_count, 7);
    assert_eq!(d2.builtin_version, 2);
}

// ---------------------------------------------------------------------------
// History — loaded through open_history, the same helper the app calls in
// `AppState::new` (lib.rs:401).
// ---------------------------------------------------------------------------

#[test]
fn history_v0_1_0_loads() {
    let (_tmp, path) = load_fixture("history-v0.1.0.json", "dictation.json");
    let (_store, h) = open_history(path.parent().expect("parent"));

    assert_eq!(h.entries.len(), 2);
    assert_eq!(h.entries[0].text, "Deployteletype steht wieder.");
    // v0.1.0 entries never had durationMs: it must come back as None, not 0
    // and not a load failure.
    assert!(h.entries[0].duration_ms.is_none());
    assert!(h.entries[1].duration_ms.is_none());
    assert_eq!(
        h.entries[0]
            .context
            .as_ref()
            .map(|c| c.application_name.clone()),
        Some("Gmail".to_string())
    );
}

#[test]
fn history_v0_2_0_loads() {
    let (_tmp, path) = load_fixture("history-v0.2.0.json", "dictation.json");
    let (_store, h) = open_history(path.parent().expect("parent"));

    assert_eq!(h.entries.len(), 2);
    assert_eq!(h.entries[0].duration_ms, Some(4200));
    assert_eq!(h.entries[1].duration_ms, None, "cancelled take keeps null");
    assert_eq!(
        h.entries[1]
            .context
            .as_ref()
            .map(|c| c.application_name.clone()),
        Some("Notes".to_string())
    );
}

#[test]
fn history_v0_2_6_loads() {
    let (_tmp, path) = load_fixture("history-v0.2.6.json", "dictation.json");
    let (_store, h) = open_history(path.parent().expect("parent"));

    assert_eq!(h.entries.len(), 2);
    assert_eq!(h.entries[0].duration_ms, Some(7123));
    assert_eq!(h.entries[1].duration_ms, Some(1500));
}

#[test]
fn history_round_trip_v0_1_0() {
    let (_tmp, path) = load_fixture("history-v0.1.0.json", "dictation.json");
    let dir = path.parent().expect("parent").to_path_buf();
    let (store, h) = open_history(&dir);
    store.save(&h).expect("re-save with today's writer");

    let (_store2, h2) = open_history(&dir);
    assert_eq!(h2.entries.len(), 2);
    assert_eq!(h2.entries[0].text, "Deployteletype steht wieder.");
    // Missing duration stays missing (None), never coerced to 0.
    assert!(h2.entries[0].duration_ms.is_none());
    assert!(h2.entries[1].duration_ms.is_none());
    // Context survives the round trip.
    assert_eq!(
        h2.entries[0]
            .context
            .as_ref()
            .map(|c| c.application_name.clone()),
        Some("Gmail".to_string())
    );
}

#[test]
fn history_round_trip_v0_2_0() {
    let (_tmp, path) = load_fixture("history-v0.2.0.json", "dictation.json");
    let dir = path.parent().expect("parent").to_path_buf();
    let (store, h) = open_history(&dir);
    store.save(&h).expect("re-save with today's writer");

    let (_store2, h2) = open_history(&dir);
    assert_eq!(h2.entries.len(), 2);
    assert_eq!(h2.entries[0].duration_ms, Some(4200));
    assert_eq!(h2.entries[1].duration_ms, None);
}

#[test]
fn history_round_trip_v0_2_6() {
    let (_tmp, path) = load_fixture("history-v0.2.6.json", "dictation.json");
    let dir = path.parent().expect("parent").to_path_buf();
    let (store, h) = open_history(&dir);
    store.save(&h).expect("re-save with today's writer");

    let (_store2, h2) = open_history(&dir);
    assert_eq!(h2.entries.len(), 2);
    assert_eq!(h2.entries[0].duration_ms, Some(7123));
    assert_eq!(h2.entries[1].duration_ms, Some(1500));
}
