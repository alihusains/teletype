//! Personalization: a compact, structured local user profile.
//!
//! Not a growing memory string. Two kinds of preferences:
//!
//! - **Explicit** — stated by the user, highest priority, always applied.
//! - **Learned** — derived locally by comparing AI output with the user's
//!   final edit, with confidence `Weak → Medium → Strong`.
//!
//! Retrieval builds a small *preference packet* (see `packet.rs`) so the model
//! only ever sees the preferences relevant to the current context.

pub mod learn;
pub mod packet;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Where a preference applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum PreferenceScope {
    #[default]
    Global,
    /// Only in one application category (e.g. email).
    AppType(crate::context::AppType),
}

/// How confident we are in a learned preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Confidence {
    Weak,
    Medium,
    Strong,
}

impl Confidence {
    /// How many consistent observations are needed for each level.
    pub fn levels() -> [u32; 3] {
        [2, 4, 8]
    }

    pub fn from_count(count: u32) -> Option<Self> {
        let [weak, medium, strong] = Self::levels();
        if count >= strong {
            Some(Self::Strong)
        } else if count >= medium {
            Some(Self::Medium)
        } else if count >= weak {
            Some(Self::Weak)
        } else {
            None
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Confidence::Weak => "learned (early)",
            Confidence::Medium => "learned",
            Confidence::Strong => "learned (consistent)",
        }
    }
}

/// One preference, explicit or learned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preference {
    #[serde(default)]
    pub id: String,
    /// User-facing description, e.g. "Prefer 'Hi' over 'Dear' in greetings".
    #[serde(default)]
    pub description: String,
    /// The short phrase injected into the prompt packet, e.g. "use 'Hi' instead of 'Dear'".
    #[serde(default)]
    pub phrase: String,
    #[serde(default)]
    pub explicit: bool,
    #[serde(default)]
    pub scope: PreferenceScope,
    /// Observation count (learned only).
    #[serde(default)]
    pub count: u32,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    /// For learned terminology preferences only: the word the AI produced
    /// ("from" in the description). Populated by the learning extractor; used
    /// to attach the learning to the dictionary word (D006/T5.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learned_from: Option<String>,
    /// For learned terminology preferences only: the word the user typed
    /// ("to"), i.e. the canonical spelling to add to the dictionary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learned_to: Option<String>,
}

impl Preference {
    pub fn new_explicit(description: impl Into<String>, phrase: impl Into<String>) -> Self {
        let now = crate::storage::now_ms();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            description: description.into(),
            phrase: phrase.into(),
            explicit: true,
            scope: PreferenceScope::Global,
            count: 0,
            created_at: now,
            updated_at: now,
            learned_from: None,
            learned_to: None,
        }
    }

    /// For learned terminology preferences: the (misheard word, canonical
    /// word) pair, when this preference was extracted from a terminology
    /// signal. Used to attach the learning to the dictionary word (D006).
    pub fn learned_correction(&self) -> Option<(&str, &str)> {
        match (&self.learned_from, &self.learned_to) {
            (Some(from), Some(to)) => Some((from, to)),
            _ => None,
        }
    }

    pub fn confidence(&self) -> Option<Confidence> {
        if self.explicit {
            Some(Confidence::Strong)
        } else {
            Confidence::from_count(self.count)
        }
    }
}

/// The stored profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UserProfile {
    pub language: String,
    pub preferences: Vec<Preference>,
    /// Whether learning from edits is turned on.
    #[serde(default = "default_true")]
    pub learn_from_edits: bool,
    /// Whether app-scoped learning is turned on.
    #[serde(default = "default_true")]
    pub learn_app_specific: bool,
    /// Whether terminology learning is turned on.
    #[serde(default = "default_true")]
    pub learn_terminology: bool,
    /// S1-mini control axes; read only when the active model is S1-mini.
    /// Defaults are the reference app's shipped values, so a user who never
    /// opens a picker sees no change.
    #[serde(default)]
    pub s1_control: crate::transforms::prompt::S1Control,
    /// Per-app ASR language overrides (P3.3): normalized app key
    /// (lowercased `application_id`, or lowercased `application_name` when
    /// the id is empty) -> concrete language code (e.g. "de"). An override
    /// for the frontmost app wins over the global language setting; an
    /// unknown app falls back to the global setting.
    #[serde(default)]
    pub app_language_overrides: BTreeMap<String, String>,
}

fn default_true() -> bool {
    true
}

impl Default for UserProfile {
    fn default() -> Self {
        Self {
            language: "en".into(),
            preferences: Vec::new(),
            learn_from_edits: true,
            learn_app_specific: true,
            learn_terminology: true,
            s1_control: Default::default(),
            app_language_overrides: BTreeMap::new(),
        }
    }
}

/// The stable key identifying an app in `UserProfile::app_language_overrides`.
///
/// Prefers the lowercased `application_id` (bundle id / process name); falls
/// back to the lowercased `application_name` when the platform exposed no id.
/// Returns `None` for an unknown app with no name — there is nothing to key
/// an override on.
pub fn app_language_key(app: &crate::context::ApplicationContext) -> Option<String> {
    let id = app.application_id.to_ascii_lowercase();
    if !id.is_empty() {
        return Some(id);
    }
    let name = app.application_name.to_ascii_lowercase();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Resolves the concrete language for the frontmost app: the per-app
/// override first, then the global profile language.
pub fn resolve_language(profile: &UserProfile, app: &crate::context::ApplicationContext) -> String {
    if let Some(key) = app_language_key(app) {
        if let Some(lang) = profile.app_language_overrides.get(&key) {
            if !lang.trim().is_empty() {
                return lang.clone();
            }
        }
    }
    profile.language.clone()
}

impl UserProfile {
    pub fn enabled(&self) -> bool {
        !self.preferences.is_empty() || self.learn_from_edits
    }

    pub fn get(&self, id: &str) -> Option<&Preference> {
        self.preferences.iter().find(|p| p.id == id)
    }

    pub fn add(&mut self, preference: Preference) {
        self.preferences.push(preference);
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.preferences.len();
        self.preferences.retain(|p| p.id != id);
        self.preferences.len() != before
    }

    /// Replaces one preference (T5.6). Preserves `created_at` and, for learned
    /// preferences, the observation `count` and `learned_from`/`learned_to`
    /// provenance: editing the wording of a learned preference must not reset
    /// its evidence. Returns true if a preference with that id was updated.
    pub fn update(&mut self, id: &str, description: &str, phrase: &str) -> bool {
        let description = description.trim();
        let phrase = phrase.trim();
        // An empty description is invalid: reject the update, leave the
        // preference untouched.
        if description.is_empty() {
            return false;
        }
        let Some(p) = self.preferences.iter_mut().find(|p| p.id == id) else {
            return false;
        };
        p.description = description.to_string();
        p.phrase = phrase.to_string();
        p.updated_at = crate::storage::now_ms();
        true
    }

    /// Master personalization switch (T5.9). When off, the preference packet is
    /// empty (the prompt gets no learned or explicit preferences) and no new
    /// preferences are learned. Enabling turns all three learning gates back
    /// on (the fine-grained toggles are set to their defaults, not restored
    /// from a saved state); the stored preferences are never deleted.
    pub fn set_personalization_enabled(&mut self, enabled: bool) {
        self.learn_from_edits = enabled;
        self.learn_app_specific = enabled;
        self.learn_terminology = enabled;
    }

    /// Removes all learned (non-explicit) preferences.
    pub fn clear_learned(&mut self) -> usize {
        let before = self.preferences.len();
        self.preferences.retain(|p| p.explicit);
        before - self.preferences.len()
    }

    /// Preferences that apply in the given context, best-first.
    ///
    /// Priority: explicit > learned; within each, global + matching app scope.
    pub fn relevant<'a>(&'a self, app: &crate::context::ApplicationContext) -> Vec<&'a Preference> {
        let mut out: Vec<&Preference> = self
            .preferences
            .iter()
            .filter(|p| matches!(p.scope, PreferenceScope::Global))
            .collect();
        if app.is_known() {
            out.extend(self.preferences.iter().filter(
                |p| matches!(&p.scope, PreferenceScope::AppType(t) if *t == app.application_type),
            ));
        }
        out.sort_by(|a, b| {
            b.explicit
                .cmp(&a.explicit)
                .then(b.count.cmp(&a.count))
                .then(a.description.cmp(&b.description))
        });
        out
    }
}

/// Baseline style hints per application category. These are *baselines*, not
/// forced styles: user preferences (explicit or learned) always win.
pub fn context_baseline(app: &crate::context::ApplicationContext) -> Vec<&'static str> {
    use crate::context::AppType;
    match app.application_type {
        AppType::Email => vec!["professional", "friendly", "concise"],
        AppType::Chat => vec!["conversational", "concise"],
        AppType::Social => vec!["natural", "concise"],
        AppType::Document => vec!["clear", "well-structured"],
        AppType::Coding => vec!["precise", "technical"],
        AppType::Terminal => vec!["precise", "concise"],
        AppType::Browser => vec!["natural"],
        AppType::Unknown => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_language_override_wins_over_global() {
        let mut profile = UserProfile {
            language: "en".into(),
            ..Default::default()
        };
        profile
            .app_language_overrides
            .insert("com.google.gmail".into(), "de".into());
        let gmail = crate::context::normalize("com.google.gmail", "Gmail");
        let slack = crate::context::normalize("com.slackmac.Slack", "Slack");
        assert_eq!(resolve_language(&profile, &gmail), "de");
        // Unknown app falls back to the global setting.
        assert_eq!(resolve_language(&profile, &slack), "en");
        // Unknown app with no id at all also falls back.
        assert_eq!(
            resolve_language(&profile, &crate::context::ApplicationContext::unknown()),
            "en"
        );
    }

    #[test]
    fn app_language_key_prefers_id_then_name() {
        let gmail = crate::context::normalize("com.google.gmail", "Gmail");
        assert_eq!(
            app_language_key(&gmail).as_deref(),
            Some("com.google.gmail")
        );
        let name_only = crate::context::normalize("", "Notes");
        assert_eq!(app_language_key(&name_only).as_deref(), Some("notes"));
        assert_eq!(
            app_language_key(&crate::context::ApplicationContext::unknown()),
            None
        );
    }

    #[test]
    fn app_language_overrides_round_trip_serde() {
        let mut profile = UserProfile::default();
        profile
            .app_language_overrides
            .insert("com.notion.id".into(), "fr".into());
        let json = serde_json::to_string(&profile).unwrap();
        let back: UserProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.app_language_overrides.get("com.notion.id").unwrap(),
            "fr"
        );
        // A document written before the field existed still deserializes.
        let legacy: UserProfile = serde_json::from_str(r#"{"language":"en"}"#).unwrap();
        assert!(legacy.app_language_overrides.is_empty());
    }

    #[test]
    fn confidence_thresholds() {
        assert_eq!(Confidence::from_count(1), None);
        assert_eq!(Confidence::from_count(2), Some(Confidence::Weak));
        assert_eq!(Confidence::from_count(4), Some(Confidence::Medium));
        assert_eq!(Confidence::from_count(8), Some(Confidence::Strong));
        assert_eq!(Confidence::from_count(99), Some(Confidence::Strong));
    }

    #[test]
    fn explicit_beats_learned_in_relevance() {
        let mut profile = UserProfile::default();
        let learned = Preference {
            id: "l1".into(),
            description: "Learned thing".into(),
            phrase: "learned phrase".into(),
            explicit: false,
            scope: PreferenceScope::Global,
            count: 8,
            created_at: 0,
            updated_at: 0,
            learned_from: None,
            learned_to: None,
        };
        let explicit = Preference::new_explicit("Keep it concise", "be concise");
        profile.add(learned);
        profile.add(explicit);
        let relevant = profile.relevant(&crate::context::ApplicationContext::unknown());
        assert_eq!(relevant[0].phrase, "be concise");
        assert_eq!(relevant[1].phrase, "learned phrase");
    }

    #[test]
    fn app_scoped_only_in_matching_context() {
        let mut profile = UserProfile::default();
        let mut p = Preference::new_explicit("Hi in emails", "use 'Hi' in greetings");
        p.scope = PreferenceScope::AppType(crate::context::AppType::Email);
        profile.add(p);

        let gmail = crate::context::normalize("com.google.gmail", "Gmail");
        let slack = crate::context::normalize("Slack", "Slack");
        assert_eq!(profile.relevant(&gmail).len(), 1);
        assert_eq!(profile.relevant(&slack).len(), 0);
    }

    #[test]
    fn clear_learned_keeps_explicit() {
        let mut profile = UserProfile::default();
        profile.add(Preference::new_explicit("a", "a"));
        profile.add(Preference {
            id: "l".into(),
            description: "b".into(),
            phrase: "b".into(),
            explicit: false,
            scope: PreferenceScope::Global,
            count: 4,
            created_at: 0,
            updated_at: 0,
            learned_from: None,
            learned_to: None,
        });
        assert_eq!(profile.clear_learned(), 1);
        assert_eq!(profile.preferences.len(), 1);
        assert!(profile.preferences[0].explicit);
    }

    #[test]
    fn baselines_per_app() {
        let gmail = crate::context::normalize("com.google.gmail", "Gmail");
        assert!(context_baseline(&gmail).contains(&"professional"));
        let slack = crate::context::normalize("Slack", "Slack");
        assert!(context_baseline(&slack).contains(&"conversational"));
        assert!(context_baseline(&crate::context::ApplicationContext::unknown()).is_empty());
    }

    #[test]
    fn t5_5_scope_serializes_both_ways() {
        // UI sends scope as "global" or {"appType":"<camelCase AppType>"}.
        let p: Preference = serde_json::from_str(
            r#"{"id":"x","description":"d","phrase":"p","explicit":true,"scope":{"appType":"coding"},"count":0}"#,
        )
        .unwrap();
        assert_eq!(
            p.scope,
            PreferenceScope::AppType(crate::context::AppType::Coding)
        );
        let p2: Preference = serde_json::from_str(
            r#"{"id":"y","description":"d","phrase":"p","explicit":true,"scope":"global"}"#,
        )
        .unwrap();
        assert_eq!(p2.scope, PreferenceScope::Global);
        // round-trip back to the wire shape the UI expects
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["scope"]["appType"], "coding");
    }

    fn learned_preference() -> Preference {
        Preference {
            id: "l1".into(),
            description: "old description".into(),
            phrase: "old phrase".into(),
            explicit: false,
            scope: PreferenceScope::AppType(crate::context::AppType::Email),
            count: 6,
            created_at: 1_000,
            updated_at: 1_000,
            learned_from: Some("margets".into()),
            learned_to: Some("Markets".into()),
        }
    }

    #[test]
    fn update_preference_preserves_count_and_provenance() {
        let mut profile = UserProfile::default();
        profile.add(learned_preference());
        let id = profile.preferences[0].id.clone();

        assert!(profile.update(&id, "new description", "new phrase"));
        let p = &profile.preferences[0];
        // Evidence and provenance survive the edit.
        assert_eq!(p.count, 6);
        assert_eq!(p.created_at, 1_000);
        assert_eq!(p.learned_from.as_deref(), Some("margets"));
        assert_eq!(p.learned_to.as_deref(), Some("Markets"));
        assert_eq!(
            p.scope,
            PreferenceScope::AppType(crate::context::AppType::Email)
        );
        assert!(!p.explicit);
        // The edited fields changed, and updated_at moved forward.
        assert_eq!(p.description, "new description");
        assert_eq!(p.phrase, "new phrase");
        assert!(p.updated_at >= 1_000);
    }

    #[test]
    fn update_preference_missing_id_returns_false() {
        let mut profile = UserProfile::default();
        profile.add(learned_preference());
        assert!(!profile.update("no-such-id", "d", "p"));
        // The profile is untouched.
        assert_eq!(profile.preferences.len(), 1);
        assert_eq!(profile.preferences[0].description, "old description");
    }

    #[test]
    fn update_preference_empty_description_is_rejected() {
        let mut profile = UserProfile::default();
        profile.add(learned_preference());
        let id = profile.preferences[0].id.clone();
        assert!(!profile.update(&id, "   ", "new phrase"));
        let p = &profile.preferences[0];
        assert_eq!(p.description, "old description");
        assert_eq!(p.phrase, "old phrase");
    }

    #[test]
    fn set_personalization_enabled_off_clears_all_three_gates() {
        let mut profile = UserProfile::default();
        profile.set_personalization_enabled(false);
        assert!(!profile.learn_from_edits);
        assert!(!profile.learn_app_specific);
        assert!(!profile.learn_terminology);
        // Stored preferences survive the switch.
        profile.add(learned_preference());
        profile.set_personalization_enabled(false);
        assert_eq!(profile.preferences.len(), 1);
    }

    #[test]
    fn set_personalization_enabled_on_restores() {
        let mut profile = UserProfile::default();
        profile.set_personalization_enabled(false);
        profile.set_personalization_enabled(true);
        assert!(profile.learn_from_edits);
        assert!(profile.learn_app_specific);
        assert!(profile.learn_terminology);
    }
}
