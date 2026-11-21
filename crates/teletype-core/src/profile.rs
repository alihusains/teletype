//! Polish Profiles: the explicit edit contract for one dictation.
//!
//! A profile answers "what may change" before the user speaks. The five states
//! are Clean, Natural, Structured, Transform and Off (see §5 of
//! `docs/research/polish-retention-strategy.md`). This module is the single
//! source of truth for:
//!
//! - budgets and the one-line prompt instruction (§6.3),
//! - per-app resolution from an existing [`ApplicationContext`] and the
//!   category table, with override precedence,
//! - effective max-change-percent precedence,
//! - model-capability fallback, and
//! - the read-only profile info payload the UI consumes.
//!
//! ## How the profile reaches the engine
//!
//! `PromptContext` and `Pipeline` are constructed in many files that T3a does
//! not own, so their field lists are frozen for this change. The resolved
//! profile therefore travels in the existing `user_override` instruction
//! channel, prefixed with a machine marker (see [`encode_instruction`] and
//! [`decode_instruction`]). A `PromptContext` built without the marker behaves
//! exactly as before; only a context built by the pipeline carries a profile.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::context::{AppType, ApplicationContext};
pub use crate::style::is_editor_model;
use crate::style::model_style;
use crate::transforms::validator::DEFAULT_MAX_CHANGE_PERCENT;

/// The user's edit contract for one polish pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PolishProfile {
    /// Fix only what is wrong: fillers, false starts, spelling, punctuation.
    Clean,
    /// Correct and readable, still the speaker's own wording.
    #[default]
    Natural,
    /// Ready to send: paragraphs, lists when enumerated, app-appropriate form.
    Structured,
    /// Explicit, user-requested rewrite (Command Mode / Rewriter). Never auto.
    Transform,
    /// No polish at all. First-class, not a model failure.
    Off,
}

impl PolishProfile {
    /// Stable key used in settings maps and on the wire.
    pub fn key(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Natural => "natural",
            Self::Structured => "structured",
            Self::Transform => "transform",
            Self::Off => "off",
        }
    }

    /// Display label (also used in fallback reasons).
    pub fn label(self) -> &'static str {
        match self {
            Self::Clean => "Clean",
            Self::Natural => "Natural",
            Self::Structured => "Structured",
            Self::Transform => "Transform",
            Self::Off => "Off",
        }
    }

    /// Contract edit budget as a percentage of words. `Transform` is explicit
    /// and has no ceiling; `Off` allows no edits.
    pub fn budget_percent(self) -> u8 {
        match self {
            Self::Clean => 15,
            Self::Natural => 30,
            Self::Structured => 45,
            Self::Transform => 100,
            Self::Off => 0,
        }
    }

    /// The one-line profile instruction from §6.3, emitted first in the
    /// instruction-capable prompt paths. `Transform` carries its own explicit
    /// user instruction and `Off` runs no model, so neither has a fixed line.
    pub fn instruction_line(self) -> Option<&'static str> {
        match self {
            Self::Clean => Some("[Profile: Clean] Fix only errors. Do not reword, reorder, or restructure. Keep every word that is not a filler, false start, or retracted phrase."),
            Self::Natural => Some("[Profile: Natural] Keep the speaker's wording and tone. Prefer minimal edits. Do not add structure unless it clearly improves clarity."),
            Self::Structured => Some("[Profile: Structured] Produce a sendable message. Lead with the point; group related ideas; use short paragraphs; use bullets only if the speaker enumerates. Keep every fact. Do not add a greeting or sign-off unless dictated."),
            Self::Transform | Self::Off => None,
        }
    }

    /// True when this profile is expressed by a prompt instruction.
    pub fn accepts_instructions(self) -> bool {
        self.instruction_line().is_some()
    }

    /// True when this profile may run the editor model (Layer 3 / §6.1).
    /// Clean and Natural stay on the cleanup path; Structured may use it.
    pub fn uses_editor_model(self) -> bool {
        matches!(self, Self::Structured | Self::Transform)
    }

    pub fn is_off(self) -> bool {
        self == Self::Off
    }

    pub fn is_transform(self) -> bool {
        self == Self::Transform
    }

    /// Parses a stable key; unknown keys return `None` so a bad settings value
    /// can be rejected rather than silently turning into a different contract.
    pub fn parse(key: &str) -> Option<Self> {
        match key.trim().to_ascii_lowercase().as_str() {
            "clean" => Some(Self::Clean),
            "natural" => Some(Self::Natural),
            "structured" => Some(Self::Structured),
            "transform" => Some(Self::Transform),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}

/// The marker prefix that carries a resolved profile in the instruction
/// channel. Kept short and unmistakable; it is stripped before the text is
/// sent to any model.
const PROFILE_MARKER_PREFIX: &str = "[polish-profile:";
const PROFILE_MARKER_SUFFIX: &str = "]\n";

/// Encodes `profile` ahead of the user's own override so the engine can read
/// the contract without changing `PromptContext`'s field list. Returns `None`
/// only when there is neither a profile marker nor an override to carry.
pub fn encode_instruction(profile: PolishProfile, user_override: Option<&str>) -> Option<String> {
    let mut out = String::with_capacity(32 + user_override.map_or(0, str::len));
    out.push_str(PROFILE_MARKER_PREFIX);
    out.push_str(profile.key());
    out.push_str(PROFILE_MARKER_SUFFIX);
    if let Some(text) = user_override.map(str::trim).filter(|t| !t.is_empty()) {
        out.push_str(text);
    }
    Some(out)
}

/// Splits the instruction channel into the encoded profile and the user's own
/// override text. A context built without a marker yields `(None, input)`, so
/// all pre-T3a callers keep byte-identical behavior.
pub fn decode_instruction(instruction: Option<&str>) -> (Option<PolishProfile>, Option<&str>) {
    let Some(raw) = instruction else {
        return (None, None);
    };
    let Some(rest) = raw.strip_prefix(PROFILE_MARKER_PREFIX) else {
        return (None, Some(raw));
    };
    let Some((key, tail)) = rest.split_once(PROFILE_MARKER_SUFFIX) else {
        return (None, Some(raw));
    };
    match PolishProfile::parse(key) {
        Some(profile) => {
            let tail = tail.trim();
            (Some(profile), (!tail.is_empty()).then_some(tail))
        }
        None => (None, Some(raw)),
    }
}

/// Convenience: the profile encoded in an instruction, defaulting to Natural
/// (the shipped global default) when no marker is present.
pub fn profile_from_instruction(instruction: Option<&str>) -> PolishProfile {
    decode_instruction(instruction).0.unwrap_or_default()
}

/// Stable category key for an app type. Used both as a settings-map key and
/// as a row key in [`APP_PROFILE_TABLE`].
pub fn category_key(app_type: AppType) -> &'static str {
    match app_type {
        AppType::Email => "email",
        AppType::Chat => "chat",
        AppType::Social => "social",
        AppType::Coding => "coding",
        AppType::Document => "document",
        AppType::Browser => "browser",
        AppType::Terminal => "terminal",
        AppType::Unknown => "other",
    }
}

/// Normalizes a bundle id / app name / category key for table and map lookup.
pub fn normalize_key(key: &str) -> String {
    key.trim().to_ascii_lowercase()
}

/// Words at or above this count make Docs/Notes take the Structured default;
/// shorter takes stay Natural (§5.3 "Structured for long, Natural for short").
pub const LONG_TAKE_WORDS: usize = 60;

/// One profile rule. `ByLength` is the only length-dependent case and exists
/// solely for docs/notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileRule {
    Fixed(PolishProfile),
    ByLength {
        short: PolishProfile,
        long: PolishProfile,
    },
}

impl ProfileRule {
    fn for_length(self, word_count: usize) -> PolishProfile {
        match self {
            Self::Fixed(p) => p,
            Self::ByLength { short, long } => {
                if word_count >= LONG_TAKE_WORDS {
                    long
                } else {
                    short
                }
            }
        }
    }
}

/// One row in the single app → profile mapping table. `key` is a normalized
/// bundle id, app name, or category key. Category rows come first so an app
/// with no explicit row still gets the right default; app-specific rows (AI
/// chat clients, which have no `AppType`) override the category default.
pub struct AppProfileRule {
    pub key: &'static str,
    pub rule: ProfileRule,
    pub note: &'static str,
}

/// The single source of truth for app defaults. Keep it table-driven: adding
/// a per-app default must not duplicate resolution logic elsewhere.
pub const APP_PROFILE_TABLE: &[AppProfileRule] = &[
    AppProfileRule {
        key: "email",
        rule: ProfileRule::Fixed(PolishProfile::Structured),
        note: "a sendable message is the intent",
    },
    AppProfileRule {
        key: "chat",
        rule: ProfileRule::Fixed(PolishProfile::Natural),
        note: "short conversational messages",
    },
    AppProfileRule {
        key: "social",
        rule: ProfileRule::Fixed(PolishProfile::Natural),
        note: "short conversational messages",
    },
    AppProfileRule {
        key: "coding",
        rule: ProfileRule::Fixed(PolishProfile::Off),
        note: "code tokens must never be polished",
    },
    AppProfileRule {
        key: "terminal",
        rule: ProfileRule::Fixed(PolishProfile::Off),
        note: "command text must never be polished",
    },
    AppProfileRule {
        key: "document",
        rule: ProfileRule::ByLength {
            short: PolishProfile::Natural,
            long: PolishProfile::Structured,
        },
        note: "long-form writing benefits from paragraphs",
    },
    AppProfileRule {
        key: "notes",
        rule: ProfileRule::ByLength {
            short: PolishProfile::Natural,
            long: PolishProfile::Structured,
        },
        note: "long notes benefit from paragraphs",
    },
    // Browser and Unknown deliberately have NO row: they fall through to the
    // user's global profile (Natural on a fresh install), which is the safe
    // default for unknown web forms and still honours an explicit choice.
    // AI chat clients have no AppType row in `context`; their names/ids get
    // explicit entries here so the table stays the only place to look.
    AppProfileRule {
        key: "chatgpt",
        rule: ProfileRule::Fixed(PolishProfile::Natural),
        note: "AI chat: clarity helps, facts and code must survive",
    },
    AppProfileRule {
        key: "com.openai.chatgpt",
        rule: ProfileRule::Fixed(PolishProfile::Natural),
        note: "AI chat: clarity helps, facts and code must survive",
    },
    AppProfileRule {
        key: "claude",
        rule: ProfileRule::Fixed(PolishProfile::Natural),
        note: "AI chat: clarity helps, facts and code must survive",
    },
    AppProfileRule {
        key: "cursor",
        rule: ProfileRule::Fixed(PolishProfile::Natural),
        note: "AI chat: clarity helps, facts and code must survive",
    },
    AppProfileRule {
        key: "github.copilot",
        rule: ProfileRule::Fixed(PolishProfile::Natural),
        note: "AI chat: clarity helps, facts and code must survive",
    },
];

fn table_profile(key: &str, word_count: usize) -> Option<PolishProfile> {
    let key = normalize_key(key);
    APP_PROFILE_TABLE
        .iter()
        .find(|row| row.key == key)
        .map(|row| row.rule.for_length(word_count))
}

/// Resolve the profile for one dictation.
///
/// Precedence (highest first):
/// 1. a per-app override in `app_overrides`, tried as bundle id, display name,
///    then category key — an explicit user override always wins;
/// 2. the app-specific row in [`APP_PROFILE_TABLE`];
/// 3. the category row (Email → Structured, Chat → Natural, IDE/Terminal →
///    Off, Docs/Notes → length-based, Browser/Unknown → Natural);
/// 4. the user's global `default_profile` (Natural on a fresh install).
///
/// An explicit per-app override therefore beats the category default, and both
/// beat the global default; this mirrors §5.3/§7.1 of the strategy document.
pub fn resolve_profile(
    app: &ApplicationContext,
    app_overrides: &BTreeMap<String, PolishProfile>,
    default_profile: PolishProfile,
    word_count: usize,
) -> PolishProfile {
    let category = category_key(app.application_type);
    let keys = [
        app.application_id.as_str(),
        app.application_name.as_str(),
        category,
    ];
    for key in keys {
        let key = normalize_key(key);
        if key.is_empty() {
            continue;
        }
        if let Some(profile) = app_overrides.get(&key) {
            return *profile;
        }
    }
    for key in keys {
        if key.is_empty() {
            continue;
        }
        if let Some(profile) = table_profile(key, word_count) {
            return profile;
        }
    }
    default_profile
}

/// The key that actually drove resolution, for the info payload and traces.
pub fn resolved_app_key(
    app: &ApplicationContext,
    app_overrides: &BTreeMap<String, PolishProfile>,
) -> String {
    let category = category_key(app.application_type);
    for key in [
        app.application_id.as_str(),
        app.application_name.as_str(),
        category,
    ] {
        let key = normalize_key(key);
        if !key.is_empty() && app_overrides.contains_key(&key) {
            return key;
        }
    }
    for key in [
        app.application_id.as_str(),
        app.application_name.as_str(),
        category,
    ] {
        let key = normalize_key(key);
        if !key.is_empty() && APP_PROFILE_TABLE.iter().any(|row| row.key == key) {
            return key;
        }
    }
    "global".to_string()
}

/// Effective change ceiling. Precedence:
/// 1. Off → 0, no edits;
/// 2. Transform → 100, the user asked for a rewrite explicitly;
/// 3. a custom ceiling that differs from the shipped default (40) **and** is
///    stricter than the profile budget → the custom value;
/// 4. otherwise the profile budget (15 / 30 / 45).
///
/// A custom limit more lenient than the profile contract never raises it: the
/// profile is the promise. A saved default value of 40 is not an explicit
/// choice, so it does not override any profile.
pub fn effective_max_change_percent(profile: PolishProfile, custom_percent: u8) -> u8 {
    let budget = profile.budget_percent();
    if profile.is_off() || profile.is_transform() {
        return budget;
    }
    if custom_percent != DEFAULT_MAX_CHANGE_PERCENT && custom_percent < budget {
        custom_percent
    } else {
        budget
    }
}

/// Which path a model takes for a given profile. Kept as stable strings so the
/// UI can render a badge without importing the engine.
pub const MODEL_CAPABILITY_CLEANUP: &str = "cleanup-only";
pub const MODEL_CAPABILITY_EDITOR: &str = "editor";
pub const MODEL_CAPABILITY_INSTRUCTION: &str = "instruction-capable";

/// The complete decision for one polish run, shared by the engine and the
/// read-only info command so they cannot disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDecision {
    pub requested: PolishProfile,
    pub effective: PolishProfile,
    pub budget_percent: u8,
    pub model_capability: &'static str,
    /// User-visible reason for a downgrade; empty when no fallback happened.
    pub reason: String,
    /// Advisory note (e.g. editor-model mismatch); not a contract downgrade.
    pub note: String,
}

impl ProfileDecision {
    pub fn fell_back(&self) -> bool {
        self.requested != self.effective
    }
}

/// Applies model capability to a resolved profile.
///
/// A cleanup-only model (S1-mini, SpeakoFlow Mini) can only run Clean or Off.
/// If the resolved profile is Natural or Structured, the contract silently
/// downgrading would be a trust breach, so the effective profile becomes Clean
/// and `reason` names the model limitation for the user. Transform is never
/// auto-selected by this function. Clean/Natural flag the editor model as an
/// advisory mismatch; actual engine routing for this is recorded in the
/// returned `note` and reported by the info command.
pub fn decide_profile(
    requested: PolishProfile,
    model_id: &str,
    custom_percent: u8,
) -> ProfileDecision {
    let style = model_style(model_id);
    let model_capability = if style.cleanup_only {
        MODEL_CAPABILITY_CLEANUP
    } else if is_editor_model(model_id) {
        MODEL_CAPABILITY_EDITOR
    } else {
        MODEL_CAPABILITY_INSTRUCTION
    };

    let mut effective = requested;
    let mut reason = String::new();
    if style.cleanup_only
        && matches!(
            requested,
            PolishProfile::Natural | PolishProfile::Structured
        )
    {
        effective = PolishProfile::Clean;
        reason = format!(
            "{model_id} can only fix errors; used Clean instead of {}.",
            requested.label()
        );
    }

    let mut note = String::new();
    if is_editor_model(model_id)
        && matches!(requested, PolishProfile::Clean | PolishProfile::Natural)
    {
        note = format!(
            "{} does not use the editor model; the {} contract keeps it on the cleanup path.",
            requested.label(),
            requested.label()
        );
    }

    ProfileDecision {
        requested,
        effective,
        budget_percent: effective_max_change_percent(effective, custom_percent),
        model_capability,
        reason,
        note,
    }
}

/// Stable payload for `get_polish_profile_info` (and the `profile` field on
/// `get_polish_prompt_info`). T3b builds the settings screen from this shape;
/// do not rename keys without a UI change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolishProfileInfo {
    /// The profile the app resolved for the current app/context.
    pub profile: String,
    /// Display label, e.g. "Structured".
    pub label: String,
    /// Key of the app/category row that won, or "global".
    pub resolved_by: String,
    /// Contract budget before any custom limit, e.g. 45.
    pub budget_percent: u8,
    /// Budget actually enforced after custom-limit precedence.
    pub effective_max_change_percent: u8,
    /// Whether the current model is cleanup-only / editor / instruction-capable.
    pub model_capability: String,
    /// Model id the decision was made for (empty when no model is selected).
    pub model_id: String,
    /// One-line user-visible fallback/advisory reason (empty when none).
    pub reason: String,
}

impl PolishProfileInfo {
    /// Builds the payload from one decision plus the resolution key.
    pub fn new(decision: &ProfileDecision, resolved_by: String, model_id: &str) -> Self {
        let message = if !decision.reason.is_empty() {
            decision.reason.clone()
        } else {
            decision.note.clone()
        };
        Self {
            profile: decision.effective.key().to_string(),
            label: decision.effective.label().to_string(),
            resolved_by,
            budget_percent: decision.effective.budget_percent(),
            effective_max_change_percent: decision.budget_percent,
            model_capability: decision.model_capability.to_string(),
            model_id: model_id.to_string(),
            reason: message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(app_type: AppType, id: &str, name: &str) -> ApplicationContext {
        ApplicationContext {
            application_id: id.into(),
            application_name: name.into(),
            application_type: app_type,
            ..Default::default()
        }
    }

    #[test]
    fn budgets_match_the_contract_table() {
        assert_eq!(PolishProfile::Clean.budget_percent(), 15);
        assert_eq!(PolishProfile::Natural.budget_percent(), 30);
        assert_eq!(PolishProfile::Structured.budget_percent(), 45);
        assert_eq!(PolishProfile::Transform.budget_percent(), 100);
        assert_eq!(PolishProfile::Off.budget_percent(), 0);
        assert_eq!(PolishProfile::default(), PolishProfile::Natural);
    }

    #[test]
    fn instruction_lines_only_for_clean_natural_structured() {
        for p in [
            PolishProfile::Clean,
            PolishProfile::Natural,
            PolishProfile::Structured,
        ] {
            let line = p
                .instruction_line()
                .unwrap_or_else(|| panic!("{} has a line", p.key()));
            assert!(line.starts_with("[Profile: "));
            assert!(p.accepts_instructions());
        }
        assert_eq!(PolishProfile::Transform.instruction_line(), None);
        assert_eq!(PolishProfile::Off.instruction_line(), None);
        assert!(!PolishProfile::Transform.accepts_instructions());
        assert!(!PolishProfile::Off.accepts_instructions());
    }

    #[test]
    fn only_structured_and_transform_may_use_the_editor_model() {
        assert!(!PolishProfile::Clean.uses_editor_model());
        assert!(!PolishProfile::Natural.uses_editor_model());
        assert!(PolishProfile::Structured.uses_editor_model());
        assert!(PolishProfile::Transform.uses_editor_model());
        assert!(is_editor_model("qwen3.5-2b"));
        assert!(!is_editor_model("qwen3.5-0.8b"));
        assert!(!is_editor_model("s1-mini"));
    }

    #[test]
    fn per_app_defaults_email_chat_ide_docs_unknown() {
        let empty = BTreeMap::new();
        assert_eq!(
            resolve_profile(
                &app(AppType::Email, "com.apple.mail", "Mail"),
                &empty,
                PolishProfile::Natural,
                10
            ),
            PolishProfile::Structured
        );
        assert_eq!(
            resolve_profile(
                &app(AppType::Chat, "com.tinyspeck.slackmacgap", "Slack"),
                &empty,
                PolishProfile::Natural,
                10
            ),
            PolishProfile::Natural
        );
        assert_eq!(
            resolve_profile(
                &app(AppType::Coding, "com.microsoft.VSCode", "Code"),
                &empty,
                PolishProfile::Structured,
                10
            ),
            PolishProfile::Off
        );
        assert_eq!(
            resolve_profile(
                &app(AppType::Terminal, "com.apple.Terminal", "Terminal"),
                &empty,
                PolishProfile::Structured,
                10
            ),
            PolishProfile::Off
        );
        assert_eq!(
            resolve_profile(
                &app(AppType::Document, "com.microsoft.Word", "Word"),
                &empty,
                PolishProfile::Natural,
                10
            ),
            PolishProfile::Natural
        );
        assert_eq!(
            resolve_profile(
                &app(AppType::Document, "com.microsoft.Word", "Word"),
                &empty,
                PolishProfile::Structured,
                LONG_TAKE_WORDS
            ),
            PolishProfile::Structured
        );
        assert_eq!(
            resolve_profile(
                &app(AppType::Unknown, "com.example.form", "Form"),
                &empty,
                PolishProfile::Natural,
                10
            ),
            PolishProfile::Natural
        );
    }

    #[test]
    fn ai_chat_apps_map_to_natural_by_name() {
        let empty = BTreeMap::new();
        assert_eq!(
            resolve_profile(
                &app(AppType::Unknown, "", "ChatGPT"),
                &empty,
                PolishProfile::Structured,
                10
            ),
            PolishProfile::Natural
        );
        assert_eq!(
            resolve_profile(
                &app(AppType::Unknown, "com.openai.chatgpt", "ChatGPT"),
                &empty,
                PolishProfile::Structured,
                10
            ),
            PolishProfile::Natural
        );
    }

    #[test]
    fn per_app_override_beats_category_default_and_global() {
        let mut overrides = BTreeMap::new();
        overrides.insert("com.apple.mail".to_string(), PolishProfile::Clean);
        assert_eq!(
            resolve_profile(
                &app(AppType::Email, "com.apple.mail", "Mail"),
                &overrides,
                PolishProfile::Structured,
                10
            ),
            PolishProfile::Clean
        );
        // A name-keyed override works when no bundle id is reported.
        let mut by_name = BTreeMap::new();
        by_name.insert("slack".to_string(), PolishProfile::Off);
        assert_eq!(
            resolve_profile(
                &app(AppType::Chat, "", "Slack"),
                &by_name,
                PolishProfile::Natural,
                10
            ),
            PolishProfile::Off
        );
        // With no override and no table row, the global default wins.
        let empty = BTreeMap::new();
        assert_eq!(
            resolve_profile(
                &app(AppType::Unknown, "", "Mystery"),
                &empty,
                PolishProfile::Structured,
                10
            ),
            PolishProfile::Structured
        );
    }

    #[test]
    fn cleanup_only_model_falls_back_to_clean_with_a_reason() {
        for model in ["s1-mini", "speakoflow-mini"] {
            for requested in [PolishProfile::Natural, PolishProfile::Structured] {
                let d = decide_profile(requested, model, 40);
                assert_eq!(
                    d.effective,
                    PolishProfile::Clean,
                    "{model}/{:?}",
                    requested.key()
                );
                assert_eq!(d.budget_percent, 15);
                assert!(d.fell_back());
                assert!(d.reason.contains(model));
                assert!(d.reason.contains("can only fix errors"));
                assert!(d.reason.contains(requested.label()));
                assert_eq!(d.model_capability, MODEL_CAPABILITY_CLEANUP);
            }
            let d = decide_profile(PolishProfile::Clean, model, 40);
            assert!(!d.fell_back());
            assert!(d.reason.is_empty());
        }
    }

    #[test]
    fn off_and_transform_are_never_fallen_back_or_auto_selected() {
        for model in ["s1-mini", "speakoflow-mini", "qwen3.5-2b", "mock"] {
            let off = decide_profile(PolishProfile::Off, model, 40);
            assert_eq!(off.effective, PolishProfile::Off);
            assert!(off.reason.is_empty());
            assert_eq!(off.budget_percent, 0);
            let transform = decide_profile(PolishProfile::Transform, model, 40);
            assert_eq!(transform.effective, PolishProfile::Transform);
            assert_eq!(transform.budget_percent, 100);
        }
    }

    #[test]
    fn editor_model_is_advisory_for_clean_natural_and_allowed_for_structured() {
        let natural = decide_profile(PolishProfile::Natural, "qwen3.5-2b", 40);
        assert_eq!(natural.effective, PolishProfile::Natural);
        assert!(natural.reason.is_empty());
        assert!(natural.note.contains("does not use the editor model"));
        assert_eq!(natural.model_capability, MODEL_CAPABILITY_EDITOR);
        let structured = decide_profile(PolishProfile::Structured, "qwen3.5-2b", 40);
        assert!(structured.note.is_empty());
        assert_eq!(structured.model_capability, MODEL_CAPABILITY_EDITOR);
    }

    #[test]
    fn custom_limit_only_wins_when_stricter_than_the_profile() {
        assert_eq!(effective_max_change_percent(PolishProfile::Natural, 40), 30);
        assert_eq!(effective_max_change_percent(PolishProfile::Natural, 10), 10);
        assert_eq!(
            effective_max_change_percent(PolishProfile::Structured, 40),
            45
        );
        assert_eq!(
            effective_max_change_percent(PolishProfile::Structured, 20),
            20
        );
        assert_eq!(
            effective_max_change_percent(PolishProfile::Structured, 90),
            45
        );
        assert_eq!(
            effective_max_change_percent(PolishProfile::Transform, 5),
            100
        );
        assert_eq!(effective_max_change_percent(PolishProfile::Off, 5), 0);
    }

    #[test]
    fn instruction_marker_round_trips_and_defaults_without_marker() {
        let encoded = encode_instruction(PolishProfile::Structured, Some("Be brief.")).unwrap();
        let (profile, override_text) = decode_instruction(Some(&encoded));
        assert_eq!(profile, Some(PolishProfile::Structured));
        assert_eq!(override_text, Some("Be brief."));
        assert_eq!(
            profile_from_instruction(Some(&encoded)),
            PolishProfile::Structured
        );

        let (none, raw) = decode_instruction(Some("Just an override"));
        assert_eq!(none, None);
        assert_eq!(raw, Some("Just an override"));
        assert_eq!(profile_from_instruction(None), PolishProfile::Natural);
    }

    #[test]
    fn info_payload_is_stable_and_reports_the_fallback_reason() {
        let d = decide_profile(PolishProfile::Structured, "s1-mini", 40);
        let info = PolishProfileInfo::new(&d, "email".into(), "s1-mini");
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["profile"], "clean");
        assert_eq!(json["budgetPercent"], 15);
        assert_eq!(json["effectiveMaxChangePercent"], 15);
        assert_eq!(json["modelCapability"], "cleanup-only");
        assert_eq!(json["resolvedBy"], "email");
        assert!(json["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("s1-mini"));
    }
}
