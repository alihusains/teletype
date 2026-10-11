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

// ---------------------------------------------------------------------------
// Polish presets: per-context capitalization/punctuation style for the Polish
// transform (Personal / Work / Email / Other), modeled on the reference
// app's style categories. These are prompt fragments, not style profiles:
// they never replace the existing profile/override machinery, they only add
// one deterministic line to the Polish prompt.
// ---------------------------------------------------------------------------

/// The three user-facing capitalization/punctuation presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PolishPreset {
    /// Sentence case + full punctuation. The shipped default.
    #[default]
    Formal,
    /// Sentence case, lighter punctuation (commas only where needed).
    Casual,
    /// No forced sentence-case capitalization, lighter punctuation.
    VeryCasual,
    /// Warmer, more emphatic register with restrained punctuation.
    Excited,
}

impl PolishPreset {
    pub const ALL: [Self; 4] = [Self::Formal, Self::Casual, Self::VeryCasual, Self::Excited];

    pub fn from_str_lenient(s: &str) -> Self {
        match s {
            "casual" => Self::Casual,
            "very-casual" => Self::VeryCasual,
            "excited" => Self::Excited,
            _ => Self::Formal,
        }
    }

    /// The prompt fragment for this preset. Meaning, named entities, numbers
    /// and URLs are protected by CORE_RULES; the fragment only steers
    /// capitalization and punctuation.
    pub fn instruction(self) -> &'static str {
        match self {
            Self::Formal => {
                "Capitalization and punctuation: start every sentence with a capital letter, use standard punctuation (periods, commas, question marks), and keep named entities, numbers, URLs and the user's own word choices exactly as written."
            }
            Self::Casual => {
                "Capitalization and punctuation: start sentences with a capital letter, but keep punctuation light: only add a comma where the sentence genuinely needs one, and prefer short periods over semicolons and dashes. Keep named entities, numbers, URLs and the user's own word choices exactly as written."
            }
            Self::VeryCasual => {
                "Capitalization and punctuation: do NOT force sentence-case capitalization (lowercase sentence starts are fine in casual messages), and keep punctuation light: only add a comma where the sentence genuinely needs one. Keep named entities, numbers, URLs and the user's own word choices exactly as written."
            }
            Self::Excited => {
                "Capitalization and punctuation: use a warmer, more enthusiastic register — you may add a brief exclamation where it genuinely fits the speaker's energy — but keep punctuation restrained (no exclamation on every sentence, no em-dashes). Keep named entities, numbers, URLs and the user's own word choices exactly as written."
            }
        }
    }

    /// The presets offered for a given context, following the reference app's
    /// category mapping: Personal offers Formal/Casual/Very casual, while
    /// Work/Email/Other offer Formal/Casual/Excited. A preset that is not in
    /// the context's set still *serializes* (backward compatibility), it just
    /// is not offered as a new choice.
    pub fn presets_for(context: PolishContext) -> &'static [Self] {
        match context {
            PolishContext::Personal => &[Self::Formal, Self::Casual, Self::VeryCasual],
            PolishContext::Work | PolishContext::Email | PolishContext::Other => {
                &[Self::Formal, Self::Casual, Self::Excited]
            }
        }
    }

    /// The wire token for a preset, e.g. `"very-casual"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Formal => "formal",
            Self::Casual => "casual",
            Self::VeryCasual => "very-casual",
            Self::Excited => "excited",
        }
    }
}

/// The four Polish style contexts the user picks presets for. The mapping to
/// detected app categories is deterministic and documented here — no content
/// sniffing, no guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PolishContext {
    /// Personal messages (chat, social, messaging apps).
    Personal,
    /// Work messages and documents (docs, notes, office apps).
    Work,
    /// Email clients.
    Email,
    /// Everything else (browsers, terminals, coding, unknown).
    Other,
}

impl PolishContext {
    pub const ALL: [Self; 4] = [Self::Personal, Self::Work, Self::Email, Self::Other];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Work => "work",
            Self::Email => "email",
            Self::Other => "other",
        }
    }

    pub fn from_str_lenient(s: &str) -> Option<Self> {
        match s {
            "personal" => Some(Self::Personal),
            "work" => Some(Self::Work),
            "email" => Some(Self::Email),
            "other" => Some(Self::Other),
            _ => None,
        }
    }

    /// The deterministic app-category → context mapping. Email is its own
    /// context; chat/social are personal; documents are work; everything
    /// else (browser, terminal, coding, unknown) is other. No app is ever
    /// classified by its window title or content.
    pub fn for_app_type(app_type: &crate::context::AppType) -> Self {
        use crate::context::AppType::*;
        match app_type {
            Email => Self::Email,
            Chat | Social => Self::Personal,
            Document => Self::Work,
            Coding | Browser | Terminal | Unknown => Self::Other,
        }
    }
}

/// Per-context preset selection, persisted in Settings. Every field is
/// serde-defaulted to `Formal` so a settings file written before this
/// feature existed loads unchanged and behaves exactly as before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[derive(Default)]
pub struct PolishStyleSettings {
    #[serde(default)]
    pub personal: PolishPreset,
    #[serde(default)]
    pub work: PolishPreset,
    #[serde(default)]
    pub email: PolishPreset,
    #[serde(default)]
    pub other: PolishPreset,
}

impl PolishStyleSettings {
    /// The preset that applies to a dictation in this app context.
    pub fn preset_for(&self, context: PolishContext) -> PolishPreset {
        match context {
            PolishContext::Personal => self.personal,
            PolishContext::Work => self.work,
            PolishContext::Email => self.email,
            PolishContext::Other => self.other,
        }
    }

    /// The preset that applies to a dictation in this app context. An
    /// explicit user choice always outranks the inferred app context: even a
    /// preset that is not currently offered for this context (e.g. a legacy
    /// settings file written when Work shared Personal's Very-casual) still
    /// applies, so no user choice is ever silently discarded. The context
    /// only decides WHICH stored choice is consulted.
    pub fn effective_preset_for(&self, context: PolishContext) -> PolishPreset {
        self.preset_for(context)
    }

    /// Sets the preset for one context; unknown context strings are ignored
    /// (the IPC layer parses defensively, mirroring S1Control).
    pub fn set(&mut self, context: PolishContext, preset: PolishPreset) {
        match context {
            PolishContext::Personal => self.personal = preset,
            PolishContext::Work => self.work = preset,
            PolishContext::Email => self.email = preset,
            PolishContext::Other => self.other = preset,
        }
    }
}

/// Resolves the active Polish preset for a dictation: the user's choice for
/// the detected app context. There is no fallback beyond the per-context
/// default (Formal) — presets are a pure prompt addition and never change
/// which model or instruction runs.
pub fn resolve_polish_preset(
    settings: &PolishStyleSettings,
    app_type: &crate::context::AppType,
) -> PolishPreset {
    settings.effective_preset_for(PolishContext::for_app_type(app_type))
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

        let dir = std::env::temp_dir().join(format!(
            "teletype-style-rt-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("styles.json");
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
        std::fs::remove_file(path).ok();
        std::fs::remove_dir(&dir).ok();
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

    // ---- Polish presets (per-context capitalization/punctuation) ----

    #[test]
    fn polish_preset_wire_tokens_and_lenient_parse() {
        for (preset, wire) in [
            (PolishPreset::Formal, "formal"),
            (PolishPreset::Casual, "casual"),
            (PolishPreset::VeryCasual, "very-casual"),
            (PolishPreset::Excited, "excited"),
        ] {
            assert_eq!(
                serde_json::to_string(&preset).unwrap(),
                format!("\"{wire}\"")
            );
            assert_eq!(PolishPreset::from_str_lenient(wire), preset);
            assert_eq!(preset.as_str(), wire);
        }
        // Unknown tokens fall back to the shipped default, never an error.
        assert_eq!(
            PolishPreset::from_str_lenient("shouting"),
            PolishPreset::Formal
        );
        assert_eq!(PolishPreset::from_str_lenient(""), PolishPreset::Formal);
    }

    #[test]
    fn polish_preset_sets_vary_by_context() {
        // Personal offers Very casual; Work/Email/Other offer Excited instead.
        let personal = PolishPreset::presets_for(PolishContext::Personal);
        assert!(personal.contains(&PolishPreset::VeryCasual));
        assert!(!personal.contains(&PolishPreset::Excited));
        for ctx in [
            PolishContext::Work,
            PolishContext::Email,
            PolishContext::Other,
        ] {
            let set = PolishPreset::presets_for(ctx);
            assert!(set.contains(&PolishPreset::Excited), "{:?}", ctx);
            assert!(!set.contains(&PolishPreset::VeryCasual), "{:?}", ctx);
            assert!(set.contains(&PolishPreset::Formal));
            assert!(set.contains(&PolishPreset::Casual));
        }
    }

    #[test]
    fn polish_preset_instructions_are_distinct_and_meaning_safe() {
        let formal = PolishPreset::Formal.instruction();
        let casual = PolishPreset::Casual.instruction();
        let very = PolishPreset::VeryCasual.instruction();
        let excited = PolishPreset::Excited.instruction();
        assert_ne!(formal, casual);
        assert_ne!(casual, very);
        assert_ne!(very, excited);
        assert_ne!(formal, excited);
        // Every preset protects the same non-negotiables.
        for frag in [formal, casual, very, excited] {
            assert!(frag.contains("named entities"));
            assert!(frag.contains("numbers"));
            assert!(frag.contains("URLs"));
        }
        // Only very-casual may waive sentence-case capitalization.
        assert!(formal.contains("start every sentence with a capital letter"));
        assert!(casual.contains("start sentences with a capital letter"));
        assert!(very.contains("do NOT force sentence-case capitalization"));
        // Excited steers warmth but keeps punctuation restrained.
        assert!(excited.contains("warm"));
        assert!(excited.contains("restrained"));
    }

    #[test]
    fn polish_context_maps_app_categories_deterministically() {
        use crate::context::AppType::*;
        assert_eq!(PolishContext::for_app_type(&Email), PolishContext::Email);
        assert_eq!(PolishContext::for_app_type(&Chat), PolishContext::Personal);
        assert_eq!(
            PolishContext::for_app_type(&Social),
            PolishContext::Personal
        );
        assert_eq!(PolishContext::for_app_type(&Document), PolishContext::Work);
        assert_eq!(PolishContext::for_app_type(&Coding), PolishContext::Other);
        assert_eq!(PolishContext::for_app_type(&Browser), PolishContext::Other);
        assert_eq!(PolishContext::for_app_type(&Terminal), PolishContext::Other);
        assert_eq!(PolishContext::for_app_type(&Unknown), PolishContext::Other);
    }

    #[test]
    fn polish_style_settings_default_is_all_formal() {
        let s = PolishStyleSettings::default();
        assert_eq!(s.preset_for(PolishContext::Personal), PolishPreset::Formal);
        assert_eq!(s.preset_for(PolishContext::Work), PolishPreset::Formal);
        assert_eq!(s.preset_for(PolishContext::Email), PolishPreset::Formal);
        assert_eq!(s.preset_for(PolishContext::Other), PolishPreset::Formal);
    }

    #[test]
    fn polish_style_settings_round_trip_and_back_compat() {
        let mut s = PolishStyleSettings::default();
        s.set(PolishContext::Email, PolishPreset::Casual);
        s.set(PolishContext::Personal, PolishPreset::VeryCasual);
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(
            json,
            r#"{"personal":"very-casual","work":"formal","email":"casual","other":"formal"}"#
        );
        let back: PolishStyleSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        // A settings file written before this feature existed has no
        // polishStyle key at all: every field must default to Formal, so
        // old data loads with no migration and no behaviour change.
        let legacy: PolishStyleSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy, PolishStyleSettings::default());
        // A partial file (e.g. user hand-edited one field) also loads.
        let partial: PolishStyleSettings =
            serde_json::from_str(r#"{"email":"very-casual"}"#).unwrap();
        assert_eq!(
            partial.preset_for(PolishContext::Email),
            PolishPreset::VeryCasual
        );
        assert_eq!(
            partial.preset_for(PolishContext::Work),
            PolishPreset::Formal
        );
    }

    #[test]
    fn resolve_polish_preset_follows_the_app_context() {
        use crate::context::AppType::*;
        let mut s = PolishStyleSettings::default();
        s.set(PolishContext::Personal, PolishPreset::VeryCasual);
        s.set(PolishContext::Email, PolishPreset::Casual);
        s.set(PolishContext::Work, PolishPreset::Excited);
        // Chat uses the personal preset, email the email preset, and a
        // document uses work (Excited here).
        assert_eq!(resolve_polish_preset(&s, &Chat), PolishPreset::VeryCasual);
        assert_eq!(resolve_polish_preset(&s, &Email), PolishPreset::Casual);
        assert_eq!(resolve_polish_preset(&s, &Document), PolishPreset::Excited);
        assert_eq!(resolve_polish_preset(&s, &Unknown), PolishPreset::Formal);
    }

    #[test]
    fn explicit_user_preset_outranks_inferred_context() {
        // Even a preset that is not currently offered for the context (a
        // legacy settings file written when Work shared Personal's
        // Very-casual set) still applies: the user's explicit choice
        // outranks the inferred app context and is never silently replaced.
        let mut s = PolishStyleSettings::default();
        s.set(PolishContext::Work, PolishPreset::VeryCasual);
        assert_eq!(
            s.effective_preset_for(PolishContext::Work),
            PolishPreset::VeryCasual,
            "an explicit user choice must survive even when the context no longer offers it"
        );
        // A preset that IS offered for the context is kept as-is.
        s.set(PolishContext::Work, PolishPreset::Excited);
        assert_eq!(
            s.effective_preset_for(PolishContext::Work),
            PolishPreset::Excited
        );
        // The context decides which stored choice is consulted: the same
        // settings under a different app context use that context's preset.
        s.set(PolishContext::Personal, PolishPreset::VeryCasual);
        assert_eq!(
            s.effective_preset_for(PolishContext::Personal),
            PolishPreset::VeryCasual
        );
        assert_eq!(
            s.effective_preset_for(PolishContext::Email),
            PolishPreset::Formal
        );
    }
}
