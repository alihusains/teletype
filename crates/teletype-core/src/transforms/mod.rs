//! Transform definitions: reusable AI instructions.

pub mod engine;
pub mod gate;
pub mod prompt;
pub mod splitter;
pub mod spoken_emoji;
pub mod validator;

use serde::{Deserialize, Serialize};

/// One transform (Polish, Professional, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformDefinition {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The instruction given to the model.
    pub instruction: String,
    /// Accelerator string, e.g. "Cmd+Shift+1" (display + registration).
    #[serde(default)]
    pub shortcut: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub built_in: bool,
    /// BCP-47-style code, e.g. "en".
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub sort_order: i32,
    /// Apply automatically after dictation when no explicit transform is chosen.
    #[serde(default)]
    pub auto_apply: bool,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

impl TransformDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        instruction: impl Into<String>,
    ) -> Self {
        let now = crate::storage::now_ms();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            description: description.into(),
            instruction: instruction.into(),
            shortcut: String::new(),
            enabled: true,
            built_in: false,
            language: "en".into(),
            sort_order: 0,
            auto_apply: false,
            created_at: now,
            updated_at: now,
        }
    }
}

/// The four built-in transforms, with production-quality instructions.
pub fn built_ins() -> Vec<TransformDefinition> {
    let now = crate::storage::now_ms();
    let mut defs = vec![
        TransformDefinition {
            id: "builtin-polish".into(),
            name: "Polish".into(),
            description: "Clean up grammar, spelling and punctuation while keeping your voice."
                .into(),
            instruction: format!("{CORE_RULES}\n{POLISH_INSTRUCTION}"),
            shortcut: String::new(),
            enabled: true,
            built_in: true,
            language: "en".into(),
            sort_order: 0,
            auto_apply: true,
            created_at: now,
            updated_at: now,
        },
        TransformDefinition {
            id: "builtin-professional".into(),
            name: "Professional".into(),
            description: "Make text clear, concise and appropriate for workplace communication."
                .into(),
            instruction: format!("{CORE_RULES}\n{PROFESSIONAL_INSTRUCTION}"),
            shortcut: String::new(),
            enabled: true,
            built_in: true,
            language: "en".into(),
            sort_order: 1,
            auto_apply: false,
            created_at: now,
            updated_at: now,
        },
        TransformDefinition {
            id: "builtin-rewriter".into(),
            name: "Rewriter".into(),
            description: "Rewrite text according to a user-defined instruction.".into(),
            instruction: format!("{CORE_RULES}\n{REWRITER_INSTRUCTION}"),
            shortcut: String::new(),
            enabled: true,
            built_in: true,
            language: "en".into(),
            sort_order: 2,
            auto_apply: false,
            created_at: now,
            updated_at: now,
        },
        TransformDefinition {
            id: "builtin-prompt-engineer".into(),
            name: "Prompt Engineer".into(),
            description: "Turn rough instructions into a clear, structured AI prompt.".into(),
            instruction: format!("{CORE_RULES}\n{PROMPT_ENGINEER_INSTRUCTION}"),
            shortcut: String::new(),
            enabled: true,
            built_in: true,
            language: "en".into(),
            sort_order: 3,
            auto_apply: false,
            created_at: now,
            updated_at: now,
        },
    ];
    defs.sort_by_key(|t| t.sort_order);
    defs
}

/// Shared non-negotiables prepended to every transform instruction.
/// Meaning always outranks style; identity fidelity is a hard requirement.
pub const CORE_RULES: &str = r#"
Rules that override any other instruction:
- Preserve meaning exactly. You may change grammar, punctuation, sentence structure, tone, verbosity and formatting.
- You must NOT change: facts, names, numbers, dates, URLs, product names, identifiers, commitments, ownership, or who performed an action.
- Never change the user's pronouns or perspective (I/we/my/our/me/us, he/she/they). Never infer gender. Never change a singular subject into plural.
- Keep every {{AUTOTEXT_N}} placeholder exactly as written, in its original position.
- Spoken lists become real lists: when the speaker announces a set of discrete items or counts them off (first, second, third), keep the lead-in sentence on its own line, then put each item on its own line starting with "- ", and drop the counting words once each item has its own line. The lead-in is the speaker's words: never drop it. Never put two items on one line or split one item across two lines. Example:
Spoken: "three jobs before we leave first call the supplier second restock the shelves third lock the back door"
Cleaned:
Three jobs before we leave:
- Call the supplier.
- Restock the shelves.
- Lock the back door.
- Restraint on lists: a single sentence is never a list, a short run inside an ordinary sentence ("bring your laptop, charger and badge") stays inside that sentence, clauses joined by "and", "but" or "so" are not a list, and connected prose about one subject stays one paragraph however many sentences it runs to. Only a clear turn to a new subject starts a new paragraph. Turning ordinary prose into a list is a mistake.
- Normalize obvious spoken formats: dates, times, numbers, currency, percentages, phone numbers, URLs and emails. Inside a URL or email, turn spoken "at", "dot", "slash", "hyphen", "dash", "underscore" and spoken digits into the symbols they stand for.
- If you are unsure whether a change preserves meaning, keep the original wording.
"#;

pub const POLISH_INSTRUCTION: &str = r#"
Polish the text: fix grammar, spelling and punctuation; improve clarity and readability. Keep the original voice, word choices and level of formality. Do not add, remove or reinterpret content.
"#;

pub const PROFESSIONAL_INSTRUCTION: &str = r#"
Rewrite the text so it is professional, natural and concise, appropriate for workplace communication. Keep the same facts, names, numbers and commitments.
"#;

pub const REWRITER_INSTRUCTION: &str = r#"
Rewrite the text according to the rewriting instruction below, preserving its meaning, facts and identifiers.

Rewriting instruction:
{{USER_INSTRUCTION}}
"#;

pub const PROMPT_ENGINEER_INSTRUCTION: &str = r#"
Convert the rough instruction into a clear, structured prompt for a capable AI assistant. Output only the final prompt. Use a short goal, then numbered requirements, then any constraints. Preserve every fact, name, number and identifier from the original.
"#;

/// The stored collection of transforms.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformStore {
    pub transforms: Vec<TransformDefinition>,
}

impl TransformStore {
    /// A fresh store seeded with the built-ins.
    pub fn with_built_ins() -> Self {
        Self {
            transforms: built_ins(),
        }
    }

    pub fn get(&self, id: &str) -> Option<&TransformDefinition> {
        self.transforms.iter().find(|t| t.id == id)
    }

    pub fn insert(&mut self, transform: TransformDefinition) -> Result<(), String> {
        if self.get(&transform.id).is_some() {
            return Err("Transform id already exists".into());
        }
        self.transforms.push(transform);
        Ok(())
    }

    pub fn update(&mut self, transform: TransformDefinition) -> Result<(), String> {
        let Some(slot) = self.transforms.iter_mut().find(|t| t.id == transform.id) else {
            return Err("Transform not found".into());
        };
        let mut updated = transform;
        updated.built_in = slot.built_in; // built-ins keep their flag
        updated.created_at = slot.created_at;
        updated.updated_at = crate::storage::now_ms();
        *slot = updated;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        // `get` already proved the transform exists, so no second lookup here.
        if let Some(t) = self.get(id) {
            if t.built_in {
                return Err("Built-in transforms can't be deleted".into());
            }
            self.transforms.retain(|t| t.id != id);
            return Ok(());
        }
        Err("Transform not found".into())
    }

    /// Restores built-in definitions (name/description/instruction/flags) while
    /// keeping user custom transforms. Returns the number of built-ins reset.
    pub fn reset_built_ins(&mut self) -> usize {
        let fresh = built_ins();
        for f in &fresh {
            if let Some(t) = self.transforms.iter_mut().find(|t| t.id == f.id) {
                t.name = f.name.clone();
                t.description = f.description.clone();
                t.instruction = f.instruction.clone();
                t.enabled = f.enabled;
                t.auto_apply = f.auto_apply;
                t.sort_order = f.sort_order;
                t.updated_at = crate::storage::now_ms();
            } else {
                self.transforms.push(f.clone());
            }
        }
        fresh.len()
    }

    /// The enabled transform marked auto-apply, if any (lowest sort_order wins).
    pub fn auto_apply(&self) -> Option<&TransformDefinition> {
        self.transforms
            .iter()
            .filter(|t| t.enabled && t.auto_apply)
            .min_by_key(|t| t.sort_order)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_ins_are_present_and_sorted() {
        let store = TransformStore::with_built_ins();
        assert_eq!(store.transforms.len(), 4);
        let names: Vec<_> = store.transforms.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            ["Polish", "Professional", "Rewriter", "Prompt Engineer"]
        );
        assert!(store.transforms.iter().all(|t| t.built_in));
        assert_eq!(
            store.auto_apply().map(|t| t.id.as_str()),
            Some("builtin-polish")
        );
    }

    #[test]
    fn cannot_delete_builtin_but_can_disable() {
        let mut store = TransformStore::with_built_ins();
        assert!(store.remove("builtin-polish").is_err());
        let mut polish = store.get("builtin-polish").unwrap().clone();
        polish.enabled = false;
        store.update(polish).unwrap();
        assert!(!store.get("builtin-polish").unwrap().enabled);
    }

    #[test]
    fn reset_defaults_restores_modified_builtin() {
        let mut store = TransformStore::with_built_ins();
        let mut polish = store.get("builtin-polish").unwrap().clone();
        polish.instruction = "hacked".into();
        polish.enabled = false;
        store.update(polish).unwrap();
        store.reset_built_ins();
        let polish = store.get("builtin-polish").unwrap();
        assert!(polish.instruction.contains("Polish the text"));
        assert!(polish.enabled);
    }

    #[test]
    fn custom_transform_roundtrip() {
        let mut store = TransformStore::with_built_ins();
        let custom = TransformDefinition::new("Slang", "Make it casual", "Make it casual.");
        let id = custom.id.clone();
        store.insert(custom).unwrap();
        assert!(store.remove(&id).is_ok());
        assert!(store.get(&id).is_none());
    }

    #[test]
    fn core_rules_are_in_every_builtin() {
        for t in TransformStore::with_built_ins().transforms.iter() {
            assert!(t.instruction.contains("Preserve meaning"), "{}", t.name);
        }
    }
}
