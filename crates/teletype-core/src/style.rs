//! Writing-style profiles: named bundles of style preferences that can be
//! applied to the transform pipeline.
//!
//! A profile is "active" when `settings.active_style_profile` matches its id.
//! The pipeline resolves the active profile's style phrases and merges them
//! into the prompt packet (transform instructions still win on conflicts —
//! they are the user's explicit per-run choice).

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
}

impl StyleProfileStore {
    pub fn with_built_ins() -> Self {
        Self {
            profiles: built_ins(),
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
}
