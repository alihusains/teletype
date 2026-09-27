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
    pub id: String,
    pub name: String,
    pub description: String,
    /// Short style phrases injected into the prompt, e.g. "be concise".
    pub style_phrases: Vec<String>,
    pub created_at: u64,
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
pub fn resolve_style_id(
    overrides: &BTreeMap<String, String>,
    app_key: &str,
    active_style: &str,
    explicit_style: &str,
) -> String {
    if !explicit_style.is_empty() {
        return explicit_style.to_string();
    }
    overrides
        .get(&app_key.trim().to_ascii_lowercase())
        .map(|s| s.as_str())
        .unwrap_or(active_style)
        .to_string()
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
        let id = resolve_style_id(&store.app_style_overrides, "gmail", "style-concise", "");
        assert_eq!(id, "style-professional");
    }

    #[test]
    fn explicit_selection_beats_app_override() {
        let mut store = StyleProfileStore::with_built_ins();
        store
            .set_app_style_override("slack", "style-casual")
            .unwrap();
        let id = resolve_style_id(
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
        let id = resolve_style_id(&store.app_style_overrides, "terminal", "style-concise", "");
        assert_eq!(id, "style-concise");
        // No global active style either → default (no style).
        let id = resolve_style_id(&store.app_style_overrides, "terminal", "", "");
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
        let id = resolve_style_id(&loaded.app_style_overrides, "slack", "style-concise", "");
        assert_eq!(id, "style-casual");
        std::fs::remove_file(path).ok();
        std::fs::remove_dir(&dir).ok();
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
}
