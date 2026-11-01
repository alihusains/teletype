//! Transform definitions: reusable AI instructions.

pub mod engine;
pub mod families;
pub mod gate;
pub mod guard;
pub mod prompt;
pub mod splitter;
pub mod spoken_emoji;
pub mod validator;

use serde::{Deserialize, Serialize};

/// How enumerated lists should be formatted in transformed output.
/// Rendered into the prompt by [`prompt::build_prompt`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ListStyle {
    /// "- item" markers (the shipped default, matching CORE_RULES).
    #[default]
    Bullets,
    /// "1. item" markers.
    Numbered,
}

impl ListStyle {
    pub const ALL: [Self; 2] = [Self::Bullets, Self::Numbered];

    pub fn from_str_lenient(s: &str) -> Self {
        match s {
            "numbered" | "numbered-list" => Self::Numbered,
            _ => Self::Bullets,
        }
    }

    /// The instruction fragment appended to the prompt for this style.
    pub fn instruction(self) -> &'static str {
        match self {
            Self::Bullets => "Format every list with a dash bullet: \"- item\".",
            Self::Numbered => {
                "Format every list with a number: \"1. item\", \"2. item\", and so on."
            }
        }
    }
}

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

/// The AI-slop floor. Appended to [`CORE_RULES`] inside the cache-stable
/// static block (see `prompt::static_prefix`), so every transform on the
/// generic prompt path carries it once instead of duplicating it into each
/// stored instruction.
///
/// The failure mode of polished dictation is not grammar, it is the filler a
/// chat model adds on its own, so this bans that vocabulary specifically. The
/// word list is the set that actually appeared in the bake-off output rather
/// than a generic guess list. Fidelity still outranks style: a banned word
/// that is the speaker's own word, a name, or a preferred term stays.
pub const ANTI_SLOP_RULES: &str = r#"
Style floor, applied after the rules above and never allowed to change meaning:
- Never use an em dash or en dash as punctuation. Use a comma, a colon, a hyphen, or split the sentence.
- Do not use these words in any form: leverage, utilize, delve, robust, seamless, holistic, streamline, elevate, unlock, harness, foster, myriad, plethora, facilitate, cutting-edge, game-changer, synergy. Say the plain word instead: use, solid, smooth, simple, improve, free, help, many, new.
- Never write: "I hope this finds you well", "don't hesitate to reach out", "it's important to note", "in conclusion", "to summarize", "at the end of the day", "let me know if you have any questions".
- Never open a sentence with "Additionally", "Moreover" or "Furthermore".
- Never add a sentence the speaker did not say: no greeting, no sign-off, no recap of what was just written.
- These bans never override fidelity. Keep names, product names, code identifiers, quoted text and anything under PREFERRED TERMS or USER PREFERENCES even when it appears on a ban list above.
"#;

pub const POLISH_INSTRUCTION: &str = r#"
Polish the text: fix grammar, spelling and punctuation; improve clarity and readability. Keep the original voice, word choices and level of formality. Do not add, remove or reinterpret content.
"#;

pub const PROFESSIONAL_INSTRUCTION: &str = r#"
You are a professional text editor who produces clear, polished writing for workplace communication.

Rewrite the provided text so it is professional, concise and natural, and appropriate for a business or workplace audience. To do this:
- Correct all grammar, spelling and punctuation errors.
- Improve structure and flow so the main point comes first and sentences are easy to follow.
- Use a formal yet warm, polite tone — never stiff, robotic or overly casual.
- Tighten wording for clarity and impact; remove filler and redundancy without losing meaning.
- Keep the original meaning, facts, names, numbers, dates, commitments and tone of intent exactly intact.

Output the revised text only. Do not add a preamble, explanations, headings, quotes or commentary, and do not wrap the result in any framing. Do not add new information or omit anything essential.
"#;

pub const REWRITER_INSTRUCTION: &str = r#"
Rewrite the text into well-structured, formal, simple yet easy-to-understand English that communicates effectively. Fix mistakes, structure the sentences properly, and add punctuation wherever needed. You may also improve the wording to make it better, while preserving its meaning, facts and identifiers.

Additional instruction from the user:
{{USER_INSTRUCTION}}
"#;

/// The one-line Prompt Engineer instruction shipped before the 10-section
/// template.
pub const LEGACY_PROMPT_ENGINEER_ONE_LINER: &str = r#"
Convert the rough instruction into a clear, structured prompt for a capable AI assistant. Output only the final prompt. Use a short goal, then numbered requirements, then any constraints. Preserve every fact, name, number and identifier from the original.
"#;

/// The full stored instruction the Prompt Engineer carried before the
/// 10-section template shipped: the shared core rules plus the old one-line
/// instruction, exactly as `built_ins()` composed it. Kept so a store seeded
/// by an older build can be told apart from a customised one at upgrade time
/// (see `TransformStore::upgrade_builtin_instruction`).
pub fn prompt_engineer_instruction_legacy() -> String {
    format!("{CORE_RULES}\n{LEGACY_PROMPT_ENGINEER_ONE_LINER}")
}

pub const PROMPT_ENGINEER_INSTRUCTION: &str = r#"
Convert the rough instruction into a structured prompt for a capable AI assistant, using exactly these ten sections, in this order, with these exact bold headings:

**Title**
(1 concise line)

**Role & stance**
(who the model is and how it should behave)

**Task**
(what the model must do)

**Context**
(only what the model needs to know)

**Inputs available**
(explicit list)

**Output requirements**
(format, structure, tone, length: only if specified, otherwise placeholders)

**Constraints / Do-nots**
(bulleted)

**Examples / References**
(include all examples verbatim)

**Execution checklist**
(short, factual verification list)

**Conflict resolution**
(only if applicable)

Rules for filling the sections:
- Preserve every fact, name, number and identifier from the spoken input. Never invent a fact.
- Anything the speaker did not say must become a placeholder of the form [FILL: what is missing] (for example [FILL: target audience], [FILL: example description]).
- "Inputs available" is a bulleted list of what the model is given.
- "Output requirements" contains only what the speaker specified; otherwise use placeholders.
- "Execution checklist" is a short list of "- [ ]" items, each a factual check the output can be verified against.
- "Conflict resolution" is N/A when nothing conflicts.
- Output only the final prompt. No preamble, no commentary.
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

    /// Upgrade path for a built-in whose stored instruction is stale.
    ///
    /// A stored instruction is treated as customised (and kept) only if it
    /// differs from *any* shipped built-in version: the current one, or a
    /// previous one listed in `previous`. This is the only way to tell a
    /// customisation apart from a user who simply never updated, because the
    /// store does not record which built-in version a row was seeded from.
    /// No change (and no `updated_at` bump) happens for a row that matches
    /// the current instruction or is already customised.
    pub fn upgrade_builtin_instruction(&mut self, id: &str, current: &str, previous: &[String]) {
        let Some(slot) = self.transforms.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if slot.instruction == current {
            return;
        }
        if previous.contains(&slot.instruction) {
            slot.instruction = current.to_string();
            slot.updated_at = crate::storage::now_ms();
        }
    }

    /// One-time migration: move the Prompt Engineer built-in from the
    /// pre-10-section one-line instruction to the current structured
    /// template, but only for rows that still carry a previously shipped
    /// instruction. A row the user customised is left untouched.
    pub fn migrate_prompt_engineer(&mut self) {
        let legacy = prompt_engineer_instruction_legacy();
        self.upgrade_builtin_instruction(
            "builtin-prompt-engineer",
            PROMPT_ENGINEER_INSTRUCTION,
            std::slice::from_ref(&legacy),
        );
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

    /// The Prompt Engineer instruction must carry all ten section headings,
    /// in the exact display order, so the model's output is a stable format.
    #[test]
    fn prompt_engineer_instruction_has_all_ten_headings_in_order() {
        let headings = [
            "**Title**",
            "**Role & stance**",
            "**Task**",
            "**Context**",
            "**Inputs available**",
            "**Output requirements**",
            "**Constraints / Do-nots**",
            "**Examples / References**",
            "**Execution checklist**",
            "**Conflict resolution**",
        ];
        let mut last = 0;
        for h in &headings {
            let pos = PROMPT_ENGINEER_INSTRUCTION
                .find(h)
                .unwrap_or_else(|| panic!("missing heading {h:?} in PROMPT_ENGINEER_INSTRUCTION"));
            assert!(pos > last, "heading {h:?} out of order");
            last = pos;
        }
    }

    #[test]
    fn upgrade_prompts_unstored_prompt_engineer_to_new_instruction() {
        let mut store = TransformStore::with_built_ins();
        // Simulate a store seeded by an older build: the row carries a
        // previous shipped instruction (not the current one, not custom).
        let mut pe = store.get("builtin-prompt-engineer").unwrap().clone();
        let previous = prompt_engineer_instruction_legacy();
        pe.instruction = previous.clone();
        store.update(pe).unwrap();

        store.upgrade_builtin_instruction(
            "builtin-prompt-engineer",
            PROMPT_ENGINEER_INSTRUCTION,
            &[previous],
        );

        let pe = store.get("builtin-prompt-engineer").unwrap();
        assert_eq!(pe.instruction, PROMPT_ENGINEER_INSTRUCTION);
    }

    #[test]
    fn upgrade_keeps_customised_prompt_engineer() {
        let mut store = TransformStore::with_built_ins();
        let mut pe = store.get("builtin-prompt-engineer").unwrap().clone();
        pe.instruction = "my own words".into();
        store.update(pe).unwrap();

        store.upgrade_builtin_instruction(
            "builtin-prompt-engineer",
            PROMPT_ENGINEER_INSTRUCTION,
            &[prompt_engineer_instruction_legacy()],
        );

        assert_eq!(
            store.get("builtin-prompt-engineer").unwrap().instruction,
            "my own words"
        );
    }

    #[test]
    fn upgrade_is_a_noop_when_already_current() {
        let mut store = TransformStore::with_built_ins();
        let before = store.get("builtin-prompt-engineer").unwrap().clone();
        store.upgrade_builtin_instruction(
            "builtin-prompt-engineer",
            PROMPT_ENGINEER_INSTRUCTION,
            &[prompt_engineer_instruction_legacy()],
        );
        let after = store.get("builtin-prompt-engineer").unwrap();
        assert_eq!(after.instruction, before.instruction);
        assert_eq!(after.updated_at, before.updated_at);
    }

    #[test]
    fn migrate_prompt_engineer_upgrades_stale_row_only() {
        let mut store = TransformStore::with_built_ins();
        // A row still carrying the old shipped instruction gets the template.
        let mut stale = store.get("builtin-prompt-engineer").unwrap().clone();
        stale.instruction = prompt_engineer_instruction_legacy();
        store.update(stale).unwrap();
        // A customised row stays customised.
        let mut other = store.get("builtin-professional").unwrap().clone();
        other.instruction = "my own words".into();
        store.update(other).unwrap();

        store.migrate_prompt_engineer();

        assert_eq!(
            store.get("builtin-prompt-engineer").unwrap().instruction,
            PROMPT_ENGINEER_INSTRUCTION
        );
        // Second run is a no-op: the row is already current.
        let before = store.get("builtin-prompt-engineer").unwrap().clone();
        store.migrate_prompt_engineer();
        let after = store.get("builtin-prompt-engineer").unwrap();
        assert_eq!(after.updated_at, before.updated_at);
    }

    #[test]
    fn prompt_engineer_instruction_encodes_fill_placeholder_rule() {
        assert!(PROMPT_ENGINEER_INSTRUCTION.contains("[FILL: "));
        assert!(PROMPT_ENGINEER_INSTRUCTION.contains("N/A"));
        assert!(PROMPT_ENGINEER_INSTRUCTION.contains("- [ ]"));
    }
}
