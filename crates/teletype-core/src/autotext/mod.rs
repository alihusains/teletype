//! AutoText: deterministic text expansion. No LLM involved.
//!
//! A trigger like `/email` expands to the user's configured value. Values are
//! **protected** around AI transforms: they are replaced with
//! `{{AUTOTEXT_n}}` placeholders before inference and restored verbatim after,
//! so a model can never alter an email address, phone number or signature.

pub mod expand;
pub mod protect;

use serde::{Deserialize, Serialize};

/// Where an entry applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AutoTextScope {
    #[default]
    Everywhere,
    /// Only in this application (matched against `ApplicationContext::application_id`,
    /// case-insensitively, or against the lowercased app name).
    Application { app_id: String },
}

/// One AutoText entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoTextEntry {
    pub id: String,
    /// The trigger, including the leading `/`.
    pub trigger: String,
    /// The exact replacement text. Sensitive — never log it.
    pub replacement: String,
    pub description: String,
    pub enabled: bool,
    pub scope: AutoTextScope,
    pub created_at: u64,
    pub updated_at: u64,
}

impl AutoTextEntry {
    pub fn new(trigger: impl Into<String>, replacement: impl Into<String>) -> Self {
        let now = crate::storage::now_ms();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            trigger: trigger.into(),
            replacement: replacement.into(),
            description: String::new(),
            enabled: true,
            scope: AutoTextScope::default(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn applies_to(&self, app: &crate::context::ApplicationContext) -> bool {
        if !self.enabled {
            return false;
        }
        match &self.scope {
            AutoTextScope::Everywhere => true,
            AutoTextScope::Application { app_id } => {
                let want = app_id.to_ascii_lowercase();
                let id = app.application_id.to_ascii_lowercase();
                let name = app.application_name.to_ascii_lowercase();
                // Match on exact id, exact name, or name as a component of the id.
                id == want
                    || name == want
                    || want.split('.').any(|part| !part.is_empty() && name == part)
            }
        }
    }
}

/// Trigger validation rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerError {
    Empty,
    MustStartWithSlash,
    TooLong { max: usize },
    InvalidCharacter { c: char },
}

impl std::fmt::Display for TriggerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TriggerError::Empty => write!(f, "Trigger must not be empty"),
            TriggerError::MustStartWithSlash => write!(f, "Trigger must start with /"),
            TriggerError::TooLong { max } => write!(f, "Trigger too long (max {max} chars)"),
            TriggerError::InvalidCharacter { c } => write!(f, "Trigger may not contain '{c}'"),
        }
    }
}

pub const MAX_TRIGGER_LEN: usize = 64;

/// Validates a trigger: `/` + letters/digits/`_`/`-`, at most 64 chars.
pub fn validate_trigger(trigger: &str) -> Result<(), TriggerError> {
    if trigger.is_empty() {
        return Err(TriggerError::Empty);
    }
    if !trigger.starts_with('/') {
        return Err(TriggerError::MustStartWithSlash);
    }
    if trigger.len() > MAX_TRIGGER_LEN {
        return Err(TriggerError::TooLong {
            max: MAX_TRIGGER_LEN,
        });
    }
    for c in trigger.chars().skip(1) {
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(TriggerError::InvalidCharacter { c });
        }
    }
    Ok(())
}

/// The stored collection of entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoTextStore {
    pub entries: Vec<AutoTextEntry>,
}

impl AutoTextStore {
    /// Adds `entry`, rejecting duplicate triggers (case-insensitive).
    pub fn insert(&mut self, entry: AutoTextEntry) -> Result<(), String> {
        let trigger = entry.trigger.to_ascii_lowercase();
        if self
            .entries
            .iter()
            .any(|e| e.trigger.to_ascii_lowercase() == trigger)
        {
            return Err(format!("A trigger '{trigger}' already exists"));
        }
        self.entries.push(entry);
        Ok(())
    }

    pub fn update(&mut self, entry: AutoTextEntry) -> Result<(), String> {
        let trigger = entry.trigger.to_ascii_lowercase();
        let pos = self
            .entries
            .iter()
            .position(|e| e.id == entry.id)
            .ok_or("Entry not found")?;
        if self.entries[pos].trigger.to_ascii_lowercase() != trigger {
            let dup = self
                .entries
                .iter()
                .any(|o| o.id != entry.id && o.trigger.to_ascii_lowercase() == trigger);
            if dup {
                return Err(format!("A trigger '{trigger}' already exists"));
            }
        }
        let mut updated = entry;
        updated.updated_at = crate::storage::now_ms();
        self.entries[pos] = updated;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() != before
    }

    pub fn get(&self, id: &str) -> Option<&AutoTextEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn find_by_trigger(&self, trigger: &str) -> Option<&AutoTextEntry> {
        let t = trigger.to_ascii_lowercase();
        self.entries
            .iter()
            .find(|e| e.trigger.to_ascii_lowercase() == t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ApplicationContext;

    fn ctx(id: &str, name: &str) -> ApplicationContext {
        ApplicationContext {
            application_id: id.into(),
            application_name: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn validate_triggers() {
        assert!(validate_trigger("/email").is_ok());
        assert!(validate_trigger("/my-signature_2").is_ok());
        assert_eq!(
            validate_trigger("email"),
            Err(TriggerError::MustStartWithSlash)
        );
        assert_eq!(validate_trigger(""), Err(TriggerError::Empty));
        assert_eq!(
            validate_trigger("/a b"),
            Err(TriggerError::InvalidCharacter { c: ' ' })
        );
        assert!(matches!(
            validate_trigger(&format!("/{}", "x".repeat(70))),
            Err(TriggerError::TooLong { .. })
        ));
    }

    #[test]
    fn duplicate_triggers_rejected() {
        let mut store = AutoTextStore::default();
        store.insert(AutoTextEntry::new("/email", "a@b.c")).unwrap();
        assert!(store.insert(AutoTextEntry::new("/EMAIL", "x@y.z")).is_err());
        // Updating the same entry to its own trigger is fine.
        let entry = store.entries[0].clone();
        store.update(entry).unwrap();
    }

    #[test]
    fn scope_everywhere_applies_everywhere() {
        let entry = AutoTextEntry::new("/name", "Ali");
        assert!(entry.applies_to(&ctx("com.google.gmail", "Gmail")));
        assert!(entry.applies_to(&ApplicationContext::unknown()));
    }

    #[test]
    fn scope_application_matches_id_or_name() {
        let mut entry = AutoTextEntry::new("/sig", "Best regards");
        entry.scope = AutoTextScope::Application {
            app_id: "com.google.gmail".into(),
        };
        assert!(entry.applies_to(&ctx("com.google.gmail", "Gmail")));
        assert!(entry.applies_to(&ctx("", "Gmail")));
        assert!(!entry.applies_to(&ctx("Slack", "Slack")));
    }

    #[test]
    fn disabled_entry_never_applies() {
        let mut entry = AutoTextEntry::new("/x", "y");
        entry.enabled = false;
        assert!(!entry.applies_to(&ApplicationContext::unknown()));
    }
}
