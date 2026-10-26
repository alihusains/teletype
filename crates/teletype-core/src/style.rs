//! Writing-style profiles: named bundles of style preferences that can be
//! applied to the transform pipeline.
//!
//! A profile is "active" when `settings.active_style_profile` matches its id.
//! The pipeline resolves the active profile's style phrases and merges them
//! into the prompt packet (transform instructions still win on conflicts —
//! they are the user's explicit per-run choice).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One style profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleProfile {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Short style phrases injected into the prompt, e.g. "be concise".
    #[serde(default)]
    pub style_phrases: Vec<String>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

impl StyleProfile {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        let now = crate::storage::now_ms();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            description: description.into(),
            style_phrases: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }
}

/// The three built-in profiles.
pub fn built_ins() -> Vec<StyleProfile> {
    let now = crate::storage::now_ms();
    [
        StyleProfile {
            id: "style-concise".into(),
            name: "Concise".into(),
            description: "Short, direct sentences. No filler, no hedging.".into(),
            style_phrases: vec![
                "be concise".into(),
                "prefer short sentences".into(),
                "avoid hedging language".into(),
            ],
            created_at: now,
            updated_at: now,
        },
        StyleProfile {
            id: "style-professional".into(),
            name: "Professional".into(),
            description: "Polished workplace tone for emails and docs.".into(),
            style_phrases: vec![
                "use a professional tone".into(),
                "avoid slang and contractions".into(),
            ],
            created_at: now,
            updated_at: now,
        },
        StyleProfile {
            id: "style-casual".into(),
            name: "Casual".into(),
            description: "Relaxed, friendly, for chat and messages.".into(),
            style_phrases: vec![
                "keep a friendly, casual tone".into(),
                "contractions are fine".into(),
            ],
            created_at: now,
            updated_at: now,
        },
    ]
    .to_vec()
}

/// The stored collection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StyleProfileStore {
    pub profiles: Vec<StyleProfile>,
    /// Per-app style overrides: normalized app key (lowercased app name) →
    /// style profile id. Persisted in the same store as the profiles.
    pub app_style_overrides: BTreeMap<String, String>,
}

impl StyleProfileStore {
    pub fn with_built_ins() -> Self {
        Self {
            profiles: built_ins(),
            app_style_overrides: BTreeMap::new(),
        }
    }

    pub fn get(&self, id: &str) -> Option<&StyleProfile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn insert(&mut self, profile: StyleProfile) -> Result<(), String> {
        if self.get(&profile.id).is_some() {
            return Err("Profile id already exists".into());
        }
        self.profiles.push(profile);
        Ok(())
    }

    pub fn update(&mut self, profile: StyleProfile) -> Result<(), String> {
        let Some(slot) = self.profiles.iter_mut().find(|p| p.id == profile.id) else {
            return Err("Profile not found".into());
        };
        let mut updated = profile;
        updated.created_at = slot.created_at;
        updated.updated_at = crate::storage::now_ms();
        *slot = updated;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        if self.get(id).is_some_and(is_builtin) {
            return Err("Built-in profiles can't be deleted".into());
        }
        let before = self.profiles.len();
        self.profiles.retain(|p| p.id != id);
        if self.profiles.len() != before {
            return Ok(());
        }
        Err("Profile not found".into())
    }

    /// Restores built-in definitions, keeping user profiles.
    pub fn reset_built_ins(&mut self) -> usize {
        let fresh = built_ins();
        for f in &fresh {
            if let Some(p) = self.profiles.iter_mut().find(|p| p.id == f.id) {
                p.name = f.name.clone();
                p.description = f.description.clone();
                p.style_phrases = f.style_phrases.clone();
                p.updated_at = crate::storage::now_ms();
            } else {
                self.profiles.push(f.clone());
            }
        }
        fresh.len()
    }

    /// The style phrases of the active profile, if any.
    pub fn active_phrases(&self, active_id: &str) -> Vec<String> {
        self.get(active_id)
            .map(|p| p.style_phrases.clone())
            .unwrap_or_default()
    }

    /// Sets a per-app style override; an empty `style_id` removes it.
    /// A non-empty `style_id` must reference an existing profile.
    pub fn set_app_style_override(&mut self, app_key: &str, style_id: &str) -> Result<(), String> {
        let key = app_key.trim().to_ascii_lowercase();
        if key.is_empty() {
            return Err("App key must not be empty".into());
        }
        if style_id.is_empty() {
            self.app_style_overrides.remove(&key);
            return Ok(());
        }
        if self.get(style_id).is_none() {
            return Err("Profile not found".into());
        }
        self.app_style_overrides.insert(key, style_id.to_string());
        Ok(())
    }

    /// The style profile id overridden for this app key, if any.
    pub fn app_style_override(&self, app_key: &str) -> Option<&str> {
        self.app_style_overrides
            .get(&app_key.trim().to_ascii_lowercase())
            .map(String::as_str)
    }
}

/// Resolves the effective style profile id for a dictation.
///
/// Resolution order:
/// 1. Explicit user selection for this dictation (`""` = none chosen).
/// 2. Per-app override for the current frontmost app.
/// 3. The user's global active style.
/// 4. Default (no style: `""`).
///
/// An override must never win over an explicit per-dictation selection.
///
/// `app_keys` is tried in order rather than a single key. The UI asks the user
/// for a bundle id (`SettingsScreen`: "App key, e.g. com.slackmac.Slack") but
/// this used to be called with `application_name`, so a user who followed the
/// hint stored `com.slackmac.slack` and resolution looked up `slack`: the
/// override was saved, listed in the UI, and never applied. Passing both and
/// trying the id first matches what `app_language_key` already does for
/// language overrides, and accepts either form.
pub fn resolve_style_id(
    overrides: &BTreeMap<String, String>,
    app_keys: &[&str],
    active_style: &str,
    explicit_style: &str,
) -> String {
    if !explicit_style.is_empty() {
        return explicit_style.to_string();
    }
    for key in app_keys {
        let key = key.trim().to_ascii_lowercase();
        if key.is_empty() {
            continue;
        }
        if let Some(s) = overrides.get(&key) {
            if !s.trim().is_empty() {
                return s.clone();
            }
        }
    }
    active_style.to_string()
}

/// Single-key convenience wrapper, for callers with only a name to go on.
pub fn resolve_style_id_for_key(
    overrides: &BTreeMap<String, String>,
    app_key: &str,
    active_style: &str,
    explicit_style: &str,
) -> String {
    resolve_style_id(overrides, &[app_key], active_style, explicit_style)
}

/// The default style to use for a given app category when the user has not
/// set a manual per-app override. Unknown/None falls back to `active_style`.
pub fn default_style_for_app(app_type: &crate::context::AppType, active_style: &str) -> String {
    use crate::context::AppType::*;
    match app_type {
        Email | Document => "style-professional".to_string(),
        Chat | Social => "style-casual".to_string(),
        Coding => "style-concise".to_string(),
        Browser | Terminal | Unknown => active_style.to_string(),
    }
}

fn is_builtin(p: &StyleProfile) -> bool {
    matches!(
        p.id.as_str(),
        "style-concise" | "style-professional" | "style-casual"
    )
}

/// How a model is told about the user's writing style.
///
/// The Style screen used to re-derive this from hard-coded model ids, so
/// `SpeakoFlow Mini` (which ignores style phrases exactly like `S1-mini` does)
/// rendered as a phrase-capable model and the user could edit styles that never
/// reach the prompt. One table, read by the engine and the UI alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StyleChannel {
    /// Its editing behaviour is fixed in the weights. Neither the style
    /// phrases nor the Tone / Structure / Context controls reach it.
    FixedBehaviour,
    /// Reads the Tone / Structure / Context control line on Polish.
    ControlLine,
    /// A general instruction follower: the active style's phrases are injected
    /// into the prompt, so every style has a visible effect.
    StylePhrases,
}

/// What one polish model does with the user's style choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStyle {
    pub channel: StyleChannel,
    /// Polish sends this model a fixed card prompt instead of the transform's
    /// own instruction, so polishing does not change with the chosen style.
    pub polishes_from_card: bool,
    /// The model cannot follow another preset's instruction at all: only
    /// Polish runs. See [`crate::transforms::engine`]'s `ModelNotCapable` skip.
    pub cleanup_only: bool,
}

/// The style behaviour of one model id. Unknown and empty ids are treated as
/// general instruction followers, which is the safe default: the worst case is
/// a phrase the model ignores, not a control band we refuse to show.
pub fn model_style(model_id: &str) -> ModelStyle {
    use StyleChannel::*;
    match model_id {
        "s1-mini" => ModelStyle {
            channel: ControlLine,
            polishes_from_card: true,
            cleanup_only: true,
        },
        "qwen3.5-2b" => ModelStyle {
            channel: ControlLine,
            polishes_from_card: true,
            cleanup_only: false,
        },
        "speakoflow-mini" => ModelStyle {
            channel: FixedBehaviour,
            polishes_from_card: true,
            cleanup_only: true,
        },
        _ => ModelStyle {
            channel: StylePhrases,
            polishes_from_card: false,
            cleanup_only: false,
        },
    }
}

impl ModelStyle {
    /// Whether the styles list can change what the model writes. A control-line
    /// model that runs nothing but Polish cannot: its prompt never carries the
    /// phrases.
    pub fn styles_apply(&self) -> bool {
        self.channel != StyleChannel::FixedBehaviour && !self.cleanup_only
    }

    /// Whether the Tone / Structure / Context controls are worth showing.
    pub fn reads_control_line(&self) -> bool {
        self.channel == StyleChannel::ControlLine
    }

    /// Whether comparing two styles on Polish can produce two different
    /// answers. A card model's Polish ignores the style, so both panes would
    /// come back the same.
    pub fn compare_is_meaningful(&self) -> bool {
        !self.polishes_from_card
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_ins_present_and_active_phrases_resolve() {
        let store = StyleProfileStore::with_built_ins();
        assert_eq!(store.profiles.len(), 3);
        assert_eq!(store.active_phrases("style-concise").len(), 3);
        assert!(store.active_phrases("nope").is_empty());
    }

    #[test]
    fn builtin_cannot_be_deleted_custom_can() {
        let mut store = StyleProfileStore::with_built_ins();
        assert!(store.remove("style-concise").is_err());
        let custom = StyleProfile::new("My Style", "d");
        let id = custom.id.clone();
        store.insert(custom).unwrap();
        assert!(store.remove(&id).is_ok());
    }

    #[test]
    fn reset_restores_modified_builtin() {
        let mut store = StyleProfileStore::with_built_ins();
        let mut p = store.get("style-casual").unwrap().clone();
        p.style_phrases.clear();
        store.update(p).unwrap();
        store.reset_built_ins();
        assert!(!store.get("style-casual").unwrap().style_phrases.is_empty());
    }

    #[test]
    fn app_override_applies_when_app_is_frontmost() {
        let mut store = StyleProfileStore::with_built_ins();
        store
            .set_app_style_override("Gmail", "style-professional")
            .unwrap();
        // Global active is concise; Gmail is frontmost → professional wins.
        let id = resolve_style_id_for_key(&store.app_style_overrides, "gmail", "style-concise", "");
        assert_eq!(id, "style-professional");
    }

    #[test]
    fn explicit_selection_beats_app_override() {
        let mut store = StyleProfileStore::with_built_ins();
        store
            .set_app_style_override("slack", "style-casual")
            .unwrap();
        let id = resolve_style_id_for_key(
            &store.app_style_overrides,
            "slack",
            "style-concise",
            "style-professional",
        );
        assert_eq!(id, "style-professional");
    }

    #[test]
    fn unknown_app_falls_through_to_global_default() {
        let mut store = StyleProfileStore::with_built_ins();
        store
            .set_app_style_override("gmail", "style-professional")
            .unwrap();
        let id =
            resolve_style_id_for_key(&store.app_style_overrides, "terminal", "style-concise", "");
        assert_eq!(id, "style-concise");
        // No global active style either → default (no style).
        let id = resolve_style_id_for_key(&store.app_style_overrides, "terminal", "", "");
        assert_eq!(id, "");
    }

    #[test]
    fn overrides_survive_store_save_load_round_trip() {
        use crate::storage::JsonStore;
        let mut store = StyleProfileStore::with_built_ins();
        store
            .set_app_style_override("Slack", "style-casual")
            .unwrap();
        store
            .set_app_style_override("Gmail", "style-professional")
            .unwrap();

        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        let json_store = JsonStore::new(&dir, "styles.json");
        json_store.save(&store).unwrap();

        let loaded: StyleProfileStore =
            JsonStore::new(&dir, "styles.json").load(StyleProfileStore::with_built_ins());
        assert_eq!(loaded.app_style_override("slack"), Some("style-casual"));
        assert_eq!(
            loaded.app_style_override("gmail"),
            Some("style-professional")
        );
        let id =
            resolve_style_id_for_key(&loaded.app_style_overrides, "slack", "style-concise", "");
        assert_eq!(id, "style-casual");
    }

    #[test]
    fn default_style_maps_app_categories() {
        use crate::context::AppType::*;
        assert_eq!(
            default_style_for_app(&Email, "style-concise"),
            "style-professional"
        );
        assert_eq!(
            default_style_for_app(&Chat, "style-concise"),
            "style-casual"
        );
        assert_eq!(
            default_style_for_app(&Social, "style-concise"),
            "style-casual"
        );
        assert_eq!(
            default_style_for_app(&Coding, "style-professional"),
            "style-concise"
        );
        assert_eq!(
            default_style_for_app(&Document, "style-concise"),
            "style-professional"
        );
        // Browser/Terminal/Unknown keep the user's global default.
        assert_eq!(
            default_style_for_app(&Browser, "style-casual"),
            "style-casual"
        );
        assert_eq!(default_style_for_app(&Terminal, ""), "");
        assert_eq!(
            default_style_for_app(&Unknown, "style-professional"),
            "style-professional"
        );
    }

    #[test]
    fn category_default_precedence() {
        let mut store = StyleProfileStore::with_built_ins();
        // Email app, no manual override → category default (professional),
        // not the global active style.
        let id = resolve_style_id_for_key(
            &store.app_style_overrides,
            "gmail",
            &default_style_for_app(&crate::context::AppType::Email, "style-concise"),
            "",
        );
        assert_eq!(id, "style-professional");
        // Email app WITH a manual override → the override wins.
        store
            .set_app_style_override("gmail", "style-casual")
            .unwrap();
        let id = resolve_style_id_for_key(
            &store.app_style_overrides,
            "gmail",
            &default_style_for_app(&crate::context::AppType::Email, "style-concise"),
            "",
        );
        assert_eq!(id, "style-casual");
        // Explicit per-dictation style beats both.
        let id = resolve_style_id_for_key(
            &store.app_style_overrides,
            "gmail",
            &default_style_for_app(&crate::context::AppType::Email, "style-concise"),
            "style-professional",
        );
        assert_eq!(id, "style-professional");
    }

    #[test]
    fn unknown_app_type_keeps_active_style() {
        let mut store = StyleProfileStore::with_built_ins();
        store
            .set_app_style_override("someapp", "style-casual")
            .unwrap();
        // Regression guard: unclassified apps behave exactly as before.
        let id = resolve_style_id_for_key(
            &store.app_style_overrides,
            "someapp",
            &default_style_for_app(&crate::context::AppType::Unknown, "style-concise"),
            "",
        );
        assert_eq!(id, "style-casual");
        let id = resolve_style_id_for_key(
            &store.app_style_overrides,
            "otherapp",
            &default_style_for_app(&crate::context::AppType::Unknown, "style-concise"),
            "",
        );
        assert_eq!(id, "style-concise");
    }

    #[test]
    fn set_override_requires_existing_profile_and_empty_removes() {
        let mut store = StyleProfileStore::with_built_ins();
        assert!(store.set_app_style_override("slack", "nope").is_err());
        store
            .set_app_style_override("slack", "style-casual")
            .unwrap();
        store.set_app_style_override("slack", "").unwrap();
        assert_eq!(store.app_style_override("slack"), None);
    }

    /// The regression this whole signature change exists for.
    ///
    /// The settings UI tells the user to enter a bundle id
    /// ("App key, e.g. com.slackmac.Slack"), and every test in this module
    /// used a bare word like "slack" or "email". So resolution keyed on
    /// `application_name`, a user who followed the hint stored
    /// `com.slackmac.slack`, lookup asked for `slack`, and the override was
    /// stored, listed in the UI, and silently never applied. The tests passed
    /// because they used the same wrong shape as the code.
    ///
    /// This asserts the case the UI actually produces.
    #[test]
    fn a_bundle_id_override_resolves_against_a_bundle_id() {
        let mut store = StyleProfileStore::with_built_ins();
        store
            .set_app_style_override("com.slackmac.Slack", "style-casual")
            .unwrap();

        // The platform reports both, and the id is what the user typed.
        let by_id = resolve_style_id(
            &store.app_style_overrides,
            &["com.slackmac.Slack", "Slack"],
            "style-concise",
            "",
        );
        assert_eq!(by_id, "style-casual");

        // A platform that reports no id must still resolve by name, so an
        // override entered either way works everywhere.
        let by_name = resolve_style_id(
            &store.app_style_overrides,
            &["", "Slack"],
            "style-concise",
            "",
        );
        assert_eq!(
            by_name, "style-concise",
            "a name-only lookup cannot match a bundle-id key; that is expected \
             and is why the id is tried first"
        );

        // And the name form still works when the name is the key.
        store
            .set_app_style_override("Terminal", "style-concise")
            .unwrap();
        assert_eq!(
            resolve_style_id(&store.app_style_overrides, &["", "Terminal"], "", ""),
            "style-concise"
        );
    }

    /// An explicit per-dictation selection still outranks an override, and
    /// the multi-key form must not have weakened that.
    #[test]
    fn explicit_selection_outranks_every_override_key() {
        let mut store = StyleProfileStore::with_built_ins();
        store
            .set_app_style_override("com.apple.Terminal", "style-casual")
            .unwrap();
        let id = resolve_style_id(
            &store.app_style_overrides,
            &["com.apple.Terminal", "Terminal"],
            "style-concise",
            "style-email",
        );
        assert_eq!(id, "style-email");
    }

    /// The three card models are the whole set that ignores style phrases on
    /// Polish. SpeakoFlow Mini is the shipped default model, and it was missing
    /// from the screen's hard-coded list, so it offered styles it never reads.
    #[test]
    fn style_table_covers_every_card_model() {
        for id in ["s1-mini", "qwen3.5-2b", "speakoflow-mini"] {
            let s = model_style(id);
            assert!(s.polishes_from_card, "{id} polishes from a card prompt");
            assert!(!s.compare_is_meaningful(), "{id} cannot compare styles");
        }
        assert!(model_style("fast").compare_is_meaningful());
        assert!(model_style("").compare_is_meaningful());
    }

    /// A cleanup-only fine-tune runs nothing but Polish, so its phrases can
    /// never reach a prompt. Qwen3.5-2B is not cleanup-only: outside Polish it
    /// takes the generic instruction, phrases and all.
    #[test]
    fn only_instruction_following_models_apply_styles() {
        assert!(!model_style("s1-mini").styles_apply());
        assert!(!model_style("speakoflow-mini").styles_apply());
        assert!(model_style("qwen3.5-2b").styles_apply());
        assert!(model_style("quality").styles_apply());
        assert!(model_style("gemma2-2b").styles_apply());
    }

    #[test]
    fn control_line_shows_for_the_models_that_read_it() {
        assert!(model_style("s1-mini").reads_control_line());
        assert!(model_style("qwen3.5-2b").reads_control_line());
        assert!(!model_style("speakoflow-mini").reads_control_line());
        assert!(!model_style("fast").reads_control_line());
    }

    /// The wire shape the Style screen consumes is camelCase, like every other
    /// command payload here.
    #[test]
    fn model_style_serialises_camel_case() {
        let json = serde_json::to_value(model_style("s1-mini")).unwrap();
        assert_eq!(json["channel"], "controlLine");
        assert_eq!(json["polishesFromCard"], true);
        assert_eq!(json["cleanupOnly"], true);
    }
}
